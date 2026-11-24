---
name: create-branch
description: Use when the user asks to create a branch to start work. If a GitHub issue applies, include its number in the branch name; if there is no issue, create the branch without an issue number. Create or switch to that branch, report it, and stop before modifying source files.
---

# Create a branch

1. Determine the issue number from the user's request or conversation. If the user says there is no issue, or no issue is mentioned and none is clearly implied by the conversation, proceed without an issue number. If several issues are plausible and it is ambiguous which one applies, ask which to use and wait.
2. If there is an issue number, verify the issue exists with `gh issue view <number>` when GitHub access is available.
3. Inspect the current branch and working tree. If unrelated uncommitted changes could be affected, stop and ask how to proceed.
4. Use an existing matching branch if one is already present locally or on `origin`. Otherwise, create a branch named:
   - With an issue: `<type>/<issue-number>-<short-slug>`, such as `fix/42-login-redirect` or `docs/13-agent-workflow`.
   - Without an issue: `<type>/<short-slug>`, such as `fix/login-redirect` or `docs/agent-workflow`.
5. Report the branch name, and the issue number if there is one.

Stop after creating or switching to the branch. Do not edit source files, commit, push, create an issue, or open a pull request as part of this skill.
