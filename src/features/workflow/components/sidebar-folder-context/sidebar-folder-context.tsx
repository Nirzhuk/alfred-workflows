import { useEffect, useState } from "react";
import type { ScrolledFolder } from "../workflow-list";

/**
 * Must match `--duration-fast`, the exit animation's duration in `App.css`. A
 * shorter value here cuts the exit off mid-flight; a longer one strands the
 * outgoing label in the header after it has finished fading.
 */
const EXIT_MS = 120;

type Labels = {
  current: ScrolledFolder | null;
  /** Kept mounted only long enough to play its exit. */
  previous: ScrolledFolder | null;
};

/**
 * Names the folder whose rows are crossing the top of the sidebar scroller.
 *
 * The label enters, leaves, and swaps rather than teleporting. Both labels are
 * stacked in one grid cell so an overlapping swap never moves the header, and
 * the incoming node animates on mount — it exists only while it is current, so
 * there is no state to track for the entrance.
 *
 * A count change on the same folder is not a swap: the node keeps its key and
 * the number updates in place.
 */
export function SidebarFolderContext({
  folder,
}: {
  folder: ScrolledFolder | null;
}) {
  const [labels, setLabels] = useState<Labels>({
    current: folder,
    previous: null,
  });

  useEffect(() => {
    setLabels((state) => {
      if (
        state.current?.name === folder?.name &&
        state.current?.count === folder?.count
      ) {
        return state;
      }
      return {
        current: folder,
        previous:
          state.current && state.current.name !== folder?.name
            ? state.current
            : null,
      };
    });
  }, [folder]);

  useEffect(() => {
    if (!labels.previous) return;
    const timeout = window.setTimeout(() => {
      setLabels((state) => ({ ...state, previous: null }));
    }, EXIT_MS);
    return () => window.clearTimeout(timeout);
  }, [labels.current, labels.previous]);

  if (!labels.current && !labels.previous) return null;

  return (
    <span className="sidebar-header-context" aria-live="polite">
      {labels.previous ? (
        <span
          className="sidebar-header-context-label is-leaving"
          aria-hidden
          key={`${labels.previous.name}-leaving`}
        >
          {labels.previous.name}
          <span className="sidebar-header-context-count">
            {labels.previous.count}
          </span>
        </span>
      ) : null}
      {labels.current ? (
        <span
          className="sidebar-header-context-label"
          key={labels.current.name}
          title={labels.current.name}
        >
          {labels.current.name}
          <span className="sidebar-header-context-count">
            {labels.current.count}
          </span>
        </span>
      ) : null}
    </span>
  );
}
