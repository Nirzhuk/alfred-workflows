/**
 * Explain-once out-of-window notice (Plan 007: "The app must explain this
 * clearly on first run rather than letting them feel tricked", recommended
 * "explain once"; Plan 005 row W4: one dismissible explanation, then silence).
 *
 * Exported and tested but deliberately NOT wired into any screen yet: the
 * customer-facing lapse copy is an unapproved decision (Plan 007, "Decisions
 * that must be approved"), and Plan 008's capability list is unapproved, so
 * there is nothing for a locked build to announce honestly.
 *
 * Persistence follows the existing localStorage preference pattern (see
 * `src/features/quick-access/preferences.ts`): one string key, failures
 * ignored so a broken store can never block the app.
 */

export const UPDATE_WINDOW_NOTICE_KEY =
  "alfred:update-window-notice-dismissed";

// DRAFT — needs owner approval.
//
// No verbatim draft exists in the plans: Plan 003 defers the lapse copy to
// Plan 004 Step 2, and RECONCILIATION-003-004-005 records that no target text
// was ever written ("005 Matrix F consistency check has no target text"). The
// wording below is assembled from the plans' own sentences and MUST be
// approved or replaced by the owner before this module is shown anywhere.
export const UPDATE_WINDOW_NOTICE_TITLE = "Your update window has closed";

// DRAFT — needs owner approval.
export const UPDATE_WINDOW_NOTICE_BODY = [
  "This Alfred build was released after your update window ended.",
  "",
  "Alfred runs exactly as before. Your workflows, memories, schedules, and all",
  "local data stay intact, and every feature you paid for keeps working on",
  "this install. Only newer releases fall outside your update window until",
  "you renew.",
  "",
  "Downloading was never the boundary; running a newer build is. You can still",
  "download any release at any time.",
].join("\n");

type StorageLike = Pick<Storage, "getItem" | "setItem">;

/** Whether the out-of-window explanation is still owed to this user. */
export function readUpdateWindowNoticeDismissed(
  storage: StorageLike,
): boolean {
  try {
    return storage.getItem(UPDATE_WINDOW_NOTICE_KEY) === "1";
  } catch {
    // An unreadable store means we have not recorded a dismissal.
    return false;
  }
}

/** Record that the user has seen the explanation. Once per build: the key has
 * no expiry, because repeating the notice would be nagging (Plan 005 W4:
 * "then silence"). */
export function dismissUpdateWindowNotice(storage: StorageLike): void {
  try {
    storage.setItem(UPDATE_WINDOW_NOTICE_KEY, "1");
  } catch {
    /* Preferences remain unavailable; the notice may show again. */
  }
}

