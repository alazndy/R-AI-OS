//! Reaper thread for detached agent children: owns the child handles so they are
//! waited on (no zombies) after the launching call has returned.

use super::*;

pub(super) const DETACHED_CHILD_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) struct DetachedChild {
    child: Child,
    completion: Option<mpsc::Sender<ExitStatus>>,
    poll_error_reported: bool,
}

pub(super) static DETACHED_CHILD_REAPER: LazyLock<Mutex<Option<mpsc::Sender<DetachedChild>>>> =
    LazyLock::new(|| Mutex::new(None));

pub(super) fn submit_detached_child(child: Child) -> Result<(), String> {
    submit_detached_child_request(DetachedChild {
        child,
        completion: None,
        poll_error_reported: false,
    })
}

#[cfg(test)]
pub(super) fn submit_detached_child_for_test(
    child: Child,
) -> Result<mpsc::Receiver<ExitStatus>, String> {
    let (completion_tx, completion_rx) = mpsc::channel();
    submit_detached_child_request(DetachedChild {
        child,
        completion: Some(completion_tx),
        poll_error_reported: false,
    })?;
    Ok(completion_rx)
}

pub(super) fn submit_detached_child_request(mut request: DetachedChild) -> Result<(), String> {
    let sender = {
        let mut reaper = match DETACHED_CHILD_REAPER.lock() {
            Ok(reaper) => reaper,
            Err(_) => {
                terminate_and_reap(&mut request.child);
                return Err("Detached child reaper state is poisoned".to_string());
            }
        };

        if reaper.is_none() {
            let (sender, receiver) = mpsc::channel();
            if let Err(error) = thread::Builder::new()
                .name("raios-child-reaper".to_string())
                .spawn(move || detached_child_reaper(receiver))
            {
                terminate_and_reap(&mut request.child);
                return Err(format!("Failed to start detached child reaper: {error}"));
            }
            *reaper = Some(sender);
        }

        match reaper.as_ref() {
            Some(sender) => sender.clone(),
            None => {
                terminate_and_reap(&mut request.child);
                return Err("Detached child reaper failed to initialize".to_string());
            }
        }
    };

    if let Err(mpsc::SendError(mut rejected)) = sender.send(request) {
        if let Ok(mut reaper) = DETACHED_CHILD_REAPER.lock() {
            *reaper = None;
        }
        terminate_and_reap(&mut rejected.child);
        return Err("Detached child reaper stopped before accepting the process".to_string());
    }

    Ok(())
}

pub(super) fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn detached_child_reaper(receiver: mpsc::Receiver<DetachedChild>) {
    let mut children = Vec::new();
    let mut disconnected = false;

    loop {
        if !disconnected {
            match receiver.recv_timeout(DETACHED_CHILD_POLL_INTERVAL) {
                Ok(child) => children.push(child),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
            }
            children.extend(receiver.try_iter());
        } else {
            thread::sleep(DETACHED_CHILD_POLL_INTERVAL);
        }

        let mut index = 0;
        while index < children.len() {
            match children[index].child.try_wait() {
                Ok(Some(status)) => {
                    let mut finished = children.swap_remove(index);
                    if let Some(completion) = finished.completion.take() {
                        let _ = completion.send(status);
                    }
                }
                Ok(None) => index += 1,
                Err(error) => {
                    if !children[index].poll_error_reported {
                        eprintln!(
                            "raios child reaper could not poll PID {}: {error}",
                            children[index].child.id()
                        );
                        children[index].poll_error_reported = true;
                    }
                    index += 1;
                }
            }
        }

        if disconnected && children.is_empty() {
            break;
        }
    }
}
