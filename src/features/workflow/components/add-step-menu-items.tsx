import {
  MenuItem,
  MenuLabel,
  MenuSub,
  MenuSubContent,
  MenuSubGroup,
  MenuSubTrigger,
} from "../../../components/menu";
import { ADD_STEP_GROUPS, type AddStepItem } from "../add-step-items";
import { AgentMark } from "./agent-mark";
import type { AgentProviderId, WorkflowNodeData } from "../types";

export type AddStepMenuHandlers = {
  onAddPrompt: (position: { x: number; y: number }) => void;
  onAddAgent: (
    provider: AgentProviderId,
    position: { x: number; y: number },
  ) => void;
  onAddChoose: (position: { x: number; y: number }) => void;
  onAddMemory: (position: { x: number; y: number }) => void;
  onAddStep: (
    type: string,
    data: WorkflowNodeData,
    position: { x: number; y: number },
  ) => void;
};

type Props = AddStepMenuHandlers & {
  getPosition: () => { x: number; y: number };
  /** Close the root menu after selecting a leaf. */
  close: () => void;
};

function selectItem(
  item: AddStepItem,
  handlers: AddStepMenuHandlers,
  position: { x: number; y: number },
) {
  if (item.kind === "prompt") handlers.onAddPrompt(position);
  else if (item.kind === "choose") handlers.onAddChoose(position);
  else if (item.kind === "memory") handlers.onAddMemory(position);
  else if (item.kind === "agent") handlers.onAddAgent(item.provider, position);
  else handlers.onAddStep(item.type, item.data, position);
}

/** Shared Add step body: Context / Agent / Sink submenus. */
export function AddStepMenuItems({
  onAddPrompt,
  onAddAgent,
  onAddChoose,
  onAddMemory,
  onAddStep,
  getPosition,
  close,
}: Props) {
  const handlers: AddStepMenuHandlers = {
    onAddPrompt,
    onAddAgent,
    onAddChoose,
    onAddMemory,
    onAddStep,
  };

  return (
    <>
      <MenuLabel>Add step</MenuLabel>
      <MenuSubGroup>
        {ADD_STEP_GROUPS.map((group) => (
          <MenuSub key={group.id}>
            <MenuSubTrigger>{group.label}</MenuSubTrigger>
            <MenuSubContent aria-label={group.label}>
              {group.items.map((item) => (
                <MenuItem
                  key={`${group.id}-${item.label}`}
                  icon={
                    item.kind === "agent" ? (
                      <AgentMark provider={item.provider} size={14} />
                    ) : undefined
                  }
                  onSelect={() => {
                    selectItem(item, handlers, getPosition());
                    close();
                  }}
                >
                  {item.label}
                </MenuItem>
              ))}
            </MenuSubContent>
          </MenuSub>
        ))}
      </MenuSubGroup>
    </>
  );
}
