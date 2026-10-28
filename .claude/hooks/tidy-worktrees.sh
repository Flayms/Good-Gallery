#!/usr/bin/env bash
# SessionStart hook (.claude/settings.json): tidies up after earlier sessions without an agent.
# - Fetches origin. Only remote-tracking refs change; local branches, the index and files stay as they are.
# - Removes worktrees whose PR was merged, with their local branch: only if they have no uncommitted changes and the
#   branch is exactly the merged PR's head, so nothing unmerged is lost. The main checkout and the worktree the session
#   starts in are never touched.
# Runs in the background and prints nothing, since hook output would land in the session's context.

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0

(
  git fetch --prune --quiet origin || exit 0
  command -v gh > /dev/null || exit 0
  main=$(git worktree list --porcelain | head -n 1 | cut -d ' ' -f 2-)
  # A session may start inside a worktree; never remove the one it runs in.
  here=$(git rev-parse --show-toplevel)
  git worktree list --porcelain |
    awk '/^worktree /{path = substr($0, 10)} /^branch refs\/heads\//{print path "\t" substr($0, 19)}' |
    while IFS=$'\t' read -r path branch; do
      [ "$path" = "$main" ] || [ "$path" = "$here" ] && continue
      [ -z "$(git -C "$path" status --porcelain)" ] || continue
      merged=$(gh pr list --head "$branch" --state merged --limit 1 --json headRefOid --jq '.[0].headRefOid')
      [ -n "$merged" ] && [ "$merged" = "$(git rev-parse "refs/heads/$branch")" ] || continue
      git worktree remove "$path" && git branch -D "$branch"
    done
) < /dev/null > /dev/null 2>&1 &

exit 0
