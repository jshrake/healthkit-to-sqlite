---
name: Bug report
about: Create a report to help us improve
title: ''
labels: bug
assignees: jshrake

---

**Describe the bug**
A clear and concise description of what the bug is. 

**To Reproduce**
If possible, please share a minimal `export.zip` or XML snippet that can reproduce the issue.

**Debug Output**
Please share the relevant log output after running with `RUST_LOG=debug`. This will provide insight into any SQL failures:

```console
RUST_LOG=debug healthkit-to-sqlite export.zip sqlite://healthkit.db
```

**Desktop (please complete the following information):**
 - OS: [e.g. Windows, MacOS, Ubuntu]

**Additional context**
Add any other context about the problem here.
