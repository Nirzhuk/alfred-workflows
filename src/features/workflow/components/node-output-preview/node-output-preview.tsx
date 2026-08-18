import { useWorkflowStore } from "../../store";

export function previewText(text: string, max = 140) {
  const [firstParagraph = ""] = text.trim().split(/\n\s*\n/, 1);
  const flat = firstParagraph.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

type Props = {
  nodeId: string;
  title: string;
};

function TerminalGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.75"
        y="2.25"
        width="12.5"
        height="11.5"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.25"
      />
      <path
        d="m4.25 6 2 2-2 2M8.25 10h3"
        stroke="currentColor"
        strokeWidth="1.25"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Clickable preview of what an agent/step generated for this node. */
export function NodeOutputPreview({ nodeId, title }: Props) {
  const output = useWorkflowStore((s) => s.stepOutputs[nodeId]);
  const running = useWorkflowStore(
    (s) => s.activeNodeId === nodeId && s.stepStatuses[nodeId] === "running",
  );
  const openRunPanel = useWorkflowStore((s) => s.openRunPanel);
  const openOutput = useWorkflowStore((s) => s.openOutput);

  if (!running && !output?.trim()) return null;

  return (
    <>
      {running ? (
        <button
          type="button"
          className="wf-node-activity nodrag nopan"
          aria-label={`Open live activity for ${title}`}
          data-prevent-node-double-click
          onClick={(event) => {
            event.stopPropagation();
            openRunPanel(nodeId);
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <TerminalGlyph />
          <span>Working ·</span>
          <span className="wf-node-activity-action" aria-hidden>
            Open console →
          </span>
        </button>
      ) : null}

      {output?.trim() ? (
        <button
          type="button"
          className="wf-node-output nodrag nopan"
          title="View full agent output"
          data-prevent-node-double-click
          onClick={(event) => {
            event.stopPropagation();
            openOutput({ title, body: output, nodeId });
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <span className="run-active-label">Output</span>
          <span className="wf-node-output-preview">{previewText(output)}</span>
          <span className="run-output-action">View full output →</span>
        </button>
      ) : null}
    </>
  );
}
