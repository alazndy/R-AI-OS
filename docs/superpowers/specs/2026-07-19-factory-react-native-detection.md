# Product Factory: React Native Capability Detection

## Boundary

React Native and Expo projects are classified before generic Node projects whenever `package.json`
declares `react-native` or `expo` in a dependency section. The general build detector exposes
`ProjectType::ReactNative`; its legacy command dispatch remains compatible with the Node adapter,
but reports the project type correctly and does not imply that a mobile build is safe to run.

The Product Factory `LocalProjectDetector` is strictly read-only. It parses `package.json` and
observes project markers only. It reports:

- Expo managed, Expo prebuild, or bare workflow;
- Android/iOS native-root presence;
- package manager and TypeScript markers;
- EAS configuration;
- local Android and macOS host capability;
- `not_assessed` signing readiness.

It never runs package scripts, reads credentials, infers signing state from secret files, starts
EAS, or submits a build. Provider selection and all external work remain later owner-approved,
artifact-bound Factory actions.

## Closed-testing quality profile

`/factory rn-quality <product_id>` idempotently seeds six required gates: TypeScript, public Expo
configuration, web export, high/critical dependency audit, Android device/closed-testing evidence,
and iOS device/TestFlight evidence. The command creates policy records only; every gate still needs
an owner-recorded passing check with an immutable evidence reference.
