# Desktop (.NET & C++) Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add native build and test support for C#/.NET and C++ (CMake) projects.

**Architecture:** Wrap `dotnet CLI` and `cmake/make` commands inside `src/core/build.rs`.

**Tech Stack:** Rust, dotnet CLI, CMake, Make/Ninja.

---

### Task 1: C# & C++ Detection

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Add `DotNet` and `Cpp` to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `detect_type`**
- [ ] **Step 3: Implement detection logic (`*.sln`, `*.csproj`, `CMakeLists.txt`)**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add DotNet and Cpp to ProjectType and implement project detection"
```

### Task 2: C# / .NET Integration

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Write failing tests for `build_dotnet`, `test_dotnet`, `check_dotnet_deps`**
- [ ] **Step 2: Implement wrappers for `dotnet build`, `dotnet test`, `dotnet list package --outdated`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: implement dotnet cli wrappers for build, test, and deps"
```

### Task 3: C++ (CMake) Integration

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing tests for `build_cpp` and `test_cpp`**
- [ ] **Step 2: Implement wrappers for `cmake --build` and `ctest`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement cmake wrappers for cpp build and test"
```

### Task 4: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a real C# or C++ project**
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for Desktop support"
git push origin master
```