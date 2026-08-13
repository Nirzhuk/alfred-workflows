import type { NodeTypes } from "@xyflow/react";
import { AgentNode } from "../agent-node";
import { ChooseOutputNode } from "../choose-output-node";
import { MemoryNode } from "../memory-node";
import { InputNode } from "../prompt-node";
import {
  FileInjectNode,
  CustomAgentNode,
  GitHostNode,
  GitStatusNode,
  HttpNode,
  NotifyNode,
  ShellNode,
  TemplateNode,
  WriteFileNode,
} from "../utility-nodes/utility-nodes";

export const nodeTypes: NodeTypes = {
  /** Legacy type id — still used by existing graphs. */
  prompt: InputNode,
  input: InputNode,
  agent: AgentNode,
  customAgent: CustomAgentNode,
  chooseOutput: ChooseOutputNode,
  memory: MemoryNode,
  template: TemplateNode,
  fileInject: FileInjectNode,
  gitStatus: GitStatusNode,
  shell: ShellNode,
  http: HttpNode,
  notify: NotifyNode,
  writeFile: WriteFileNode,
  gitHost: GitHostNode,
};
