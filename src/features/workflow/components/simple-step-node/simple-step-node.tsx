import { Handle, Position, type NodeProps } from "@xyflow/react";
import { NodeOutputPreview } from "../node-output-preview";

type Props = {
  id: string;
  className: string;
  title: string;
  body: string;
  meta?: string;
};

/** Compact display-only step card used by Context / Action / Sink utilities. */
export function SimpleStepNode({ id, className, title, body, meta }: Props) {
  return (
    <div className={`wf-node ${className}`}>
      <Handle
        className="wf-handle"
        type="target"
        position={Position.Left}
        isConnectable
      />
      <div className="wf-node-title">{title}</div>
      <p className="wf-node-body">{body}</p>
      {meta ? <p className="wf-node-skill muted">{meta}</p> : null}
      <NodeOutputPreview nodeId={id} title={title} />
      <Handle
        className="wf-handle"
        type="source"
        position={Position.Right}
        isConnectable
      />
    </div>
  );
}

/** Truncate a one-line preview for node cards. */
export function previewLine(text: string, empty = "Not configured"): string {
  const t = text.trim();
  if (!t) return empty;
  return t.length > 72 ? `${t.slice(0, 69)}…` : t;
}

/** Satisfy React Flow NodeProps typing for simple cards. */
export type AnyNodeProps = NodeProps;
