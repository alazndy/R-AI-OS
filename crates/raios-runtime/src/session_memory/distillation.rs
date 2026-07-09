/// L2: upsert the per-day scene block digest for today and link it to its facts.
/// Returns the scene slug, or None if fact_slugs is empty or DB writes failed.
pub(super) fn upsert_scene_block(
    conn: &rusqlite::Connection,
    project_key: &str,
    fact_slugs: &[(String, &'static str, String)],
) -> Option<String> {
    if fact_slugs.is_empty() {
        return None;
    }
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let scene_slug = format!("scene-{}", date.replace('-', ""));

    // Cumulative merge: multiple syncs on the same day must accumulate facts into the
    // scene, not overwrite it — mem_upsert REPLACES the body on conflict, so we need to
    // read the existing body first and only append genuinely new lines.
    let mut lines: Vec<String> = raios_core::db::mem_get(conn, project_key, &scene_slug)
        .ok()
        .flatten()
        .map(|s| s.body.lines().map(String::from).collect())
        .unwrap_or_default();
    for (slug, t, text) in fact_slugs {
        let line = format!("- [{t}] {text} ([[{slug}]])");
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    let body = lines.join("\n");

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "project",
            slug: &scene_slug,
            title: &format!("Scene ({})", date),
            description: &format!("{} fact(s) distilled", fact_slugs.len()),
            body: &body,
            session_id: None,
            layer: 2,
        },
    )
    .ok()?;

    let scene = raios_core::db::mem_get(conn, project_key, &scene_slug).ok()??;
    for (slug, _, _) in fact_slugs {
        if let Ok(Some(fact)) = raios_core::db::mem_get(conn, project_key, slug) {
            let _ = raios_core::db::mem_lineage_add(
                conn, "item", &scene.id, "item", &fact.id, "derived_from",
            );
        }
    }
    Some(scene_slug)
}

/// L3: rebuild the project persona from L1 user/feedback facts. Deterministic, no LLM.
pub fn rebuild_persona(conn: &rusqlite::Connection, project_key: &str) -> Option<()> {
    let items = raios_core::db::mem_list(conn, project_key).ok()?;
    let mut user: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "user" && i.slug != "persona")
        .collect();
    let mut feedback: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "feedback")
        .collect();
    if user.is_empty() && feedback.is_empty() {
        return None;
    }
    // newest first, capped
    user.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    feedback.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    user.truncate(10);
    feedback.truncate(20);

    let mut body = String::new();
    if !user.is_empty() {
        body.push_str("## Background\n");
        for i in &user {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }
    if !feedback.is_empty() {
        body.push_str("\n## Working Rules\n");
        for i in &feedback {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "user",
            slug: "persona",
            title: "Persona",
            description: &format!(
                "{} background + {} rule fact(s)",
                user.len(),
                feedback.len()
            ),
            body: body.trim_end(),
            session_id: None,
            layer: 3,
        },
    )
    .ok()?;

    let persona = raios_core::db::mem_get(conn, project_key, "persona").ok()??;
    for i in user.iter().chain(feedback.iter()) {
        let _ = raios_core::db::mem_lineage_add(
            conn, "item", &persona.id, "item", &i.id, "derived_from",
        );
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fact_slug;

    #[test]
    fn scene_block_upsert_links_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // seed two L1 facts
        for (t, txt) in [("feedback", "don't use npm"), ("project", "we decided sqlite")] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }
        let slugs: Vec<(String, &'static str, String)> = vec![
            (fact_slug("feedback", "don't use npm"), "feedback", "don't use npm".into()),
            (fact_slug("project", "we decided sqlite"), "project", "we decided sqlite".into()),
        ];

        let scene_slug = upsert_scene_block(&conn, key, &slugs).unwrap();
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug).unwrap().unwrap();
        assert_eq!(scene.layer, 2);
        assert!(scene.body.contains("don't use npm"));
        assert!(scene.body.contains(&fact_slug("project", "we decided sqlite")));

        // lineage: scene → 2 fact parents
        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &scene.id).unwrap();
        assert_eq!(parents.len(), 2);
        assert!(parents.iter().all(|(k, _, r)| k == "item" && r == "derived_from"));
    }

    #[test]
    fn scene_block_accumulates_across_same_day_calls() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // seed first L1 fact and upsert the scene with only that fact
        let (t1, txt1) = ("feedback", "don't use npm");
        let slug1 = fact_slug(t1, txt1);
        raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
            project_key: key, item_type: t1, slug: &slug1, title: txt1,
            description: txt1, body: txt1, session_id: None, layer: 1,
        }).unwrap();
        let fact1_tuple: (String, &'static str, String) = (slug1.clone(), t1, txt1.into());

        let scene_slug_1 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact1_tuple)).unwrap();

        // seed a second, different L1 fact and upsert the scene again with only that fact
        let (t2, txt2) = ("project", "we decided sqlite");
        let slug2 = fact_slug(t2, txt2);
        raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
            project_key: key, item_type: t2, slug: &slug2, title: txt2,
            description: txt2, body: txt2, session_id: None, layer: 1,
        }).unwrap();
        let fact2_tuple: (String, &'static str, String) = (slug2.clone(), t2, txt2.into());

        let scene_slug_2 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact2_tuple)).unwrap();

        // same day → same scene slug
        assert_eq!(scene_slug_1, scene_slug_2);

        // both facts' text must be present in the merged body (accumulation, not overwrite)
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug_2).unwrap().unwrap();
        assert!(scene.body.contains(txt1));
        assert!(scene.body.contains(txt2));

        // calling a third time with the SAME first fact again must not duplicate its line
        let scene_slug_3 = upsert_scene_block(&conn, key, std::slice::from_ref(&fact1_tuple)).unwrap();
        assert_eq!(scene_slug_3, scene_slug_1);
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug_3).unwrap().unwrap();
        let fact_lines: Vec<&str> = scene.body.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(fact_lines.len(), 2, "expected exactly 2 deduped fact lines, got: {:?}", fact_lines);

        // lineage: even after re-passing fact1 a second time, there must still be exactly
        // 2 "derived_from" fact parents, not 3 — lineage_add is idempotent per Task 2.
        // (mem_upsert also records a separate "revision" lineage row, with parent_kind
        // "node", whenever the scene body changes between calls — that's the Task 3
        // body-versioning mechanism working as intended, not fact lineage, so it's
        // excluded here rather than asserting the raw total.)
        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &scene.id).unwrap();
        let fact_parents: Vec<_> = parents
            .iter()
            .filter(|(k, _, r)| k == "item" && r == "derived_from")
            .collect();
        assert_eq!(
            fact_parents.len(),
            2,
            "expected exactly 2 derived_from fact parents, got: {:?}",
            parents
        );
    }

    #[test]
    fn persona_assembles_from_user_and_feedback_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        for (t, txt) in [
            ("user", "ben gömülü sistem geliştiriciyim"),
            ("feedback", "don't use npm, use pnpm"),
            ("project", "we decided sqlite"), // must NOT appear in persona
        ] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }

        rebuild_persona(&conn, key).unwrap();
        let p = raios_core::db::mem_get(&conn, key, "persona").unwrap().unwrap();
        assert_eq!(p.layer, 3);
        assert!(p.body.contains("## Background"));
        assert!(p.body.contains("gömülü sistem"));
        assert!(p.body.contains("## Working Rules"));
        assert!(p.body.contains("use pnpm"));
        assert!(!p.body.contains("sqlite"));

        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &p.id).unwrap();
        assert_eq!(parents.len(), 2);
    }
}
