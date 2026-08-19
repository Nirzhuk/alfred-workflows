import type { Node, NodeProps } from "@xyflow/react";
import type { AppActionNodeData } from "../../types";
import { AppLogo } from "../../../integrations/app-logo";
import { previewLine, SimpleStepNode } from "../simple-step-node/simple-step-node";

/** "google_drive" -> "Google Drive"; empty stays empty. */
export function capitalizeAppName(providerId: string): string {
  return providerId
    .split("_")
    .filter(Boolean)
    .map((word) => word.charAt(0).toLocaleUpperCase() + word.slice(1))
    .join(" ");
}

export function AppActionNode({
  id,
  data,
}: NodeProps<Node<AppActionNodeData, "appAction">>) {
  const hasApp = Boolean(data.providerId);
  const appName = capitalizeAppName(data.providerId);
  const title =
    !hasApp
      ? data.label || "App action"
      : data.label && data.label !== appName
        ? data.label
        : appName;
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-app-action"
      icon={
        hasApp ? (
          <AppLogo
            providerId={data.providerId}
            providerName={appName}
            size={20}
          />
        ) : undefined
      }
      title={title}
      body={previewLine(data.actionId, "Choose an action")}
      meta={
        data.connectionId
          ? `${data.providerId || "App"} · connected`
          : data.providerId || "Connected app"
      }
    />
  );
}
