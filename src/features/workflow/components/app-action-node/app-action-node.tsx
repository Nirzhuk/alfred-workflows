import type { Node, NodeProps } from "@xyflow/react";
import type { AppActionNodeData } from "../../types";
import { previewLine, SimpleStepNode } from "../simple-step-node/simple-step-node";

export function AppActionNode({
  id,
  data,
}: NodeProps<Node<AppActionNodeData, "appAction">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-app-action"
      title={data.label || "App action"}
      body={previewLine(data.actionId, "Choose an action")}
      meta={
        data.connectionId
          ? `${data.providerId || "App"} · connected`
          : data.providerId || "Connected app"
      }
    />
  );
}
