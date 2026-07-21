# Product Factory Phase 10: Controlled Stage Task Graphs

## Objective

Connect an approved Factory cycle stage to the existing canonical control-plane task graph without enabling unattended execution.

## Boundary

`MaterializeStageTaskGraph` is owner-bound and idempotent per `(cycle, stage)`. It creates:

- a canonical task graph and one stage task/run contract;
- an empty executable command field, so no shell operation is implicitly authorized;
- a pending `factory_stage_execution` approval owned by the product owner;
- a `task_graph_id` reference on `cp_factory_stage_runs`.

The materialization operation does not launch an agent, execute a shell command, resolve the approval, create release evidence, or publish a build.

After the existing control-plane approval is explicitly resolved as `approved`, the owner may activate the stage. Activation updates only canonical Factory lifecycle state (`pending` to `active`) and still does not execute a shell or launch an agent.

## Local Command

```text
/factory stage-graph <cycle_id> | <stage>
/factory activate-stage <cycle_id> | <stage>
```

## Verification

Core tests assert that the graph link is persisted, the executable command is empty, a pending approval exists, and repeated materialization returns the same graph.
