import type { ReactNode } from "react";
import {
  ContextMenu,
  MenuDescription,
  MenuItem,
  MenuLabel,
  MenuSeparator,
  useContextMenuClose,
} from "../../../../components/menu";

type Props = {
  x: number;
  y: number;
  folderName: string;
  onClose: () => void;
  onCreateWorkflow: () => void;
  onRename: () => void;
  onDelete: () => void;
};

function Icon({ children }: { children: ReactNode }) {
  return children;
}

function PlusIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
      </svg>
    </Icon>
  );
}

function PencilIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path d="M9.5 3.5 12.5 6.5M3 13l.7-3.2L10.8 2.7a1.4 1.4 0 0 1 2 2L5.8 11.7 3 13Z" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" />
      </svg>
    </Icon>
  );
}

function TrashIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path d="M3.5 4.5h9M6 4.5V3.2h4v1.3M5.2 4.5l.5 8.3h4.6l.5-8.3" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </Icon>
  );
}

function FolderMenuBody({
  folderName,
  onCreateWorkflow,
  onRename,
  onDelete,
}: Omit<Props, "x" | "y" | "onClose">) {
  const close = useContextMenuClose();
  const run = (action: () => void) => {
    action();
    close();
  };

  return (
    <>
      <MenuLabel>Folder</MenuLabel>
      <MenuDescription title={folderName}>{folderName}</MenuDescription>
      <MenuItem icon={<PlusIcon />} onSelect={() => run(onCreateWorkflow)}>
        New workflow
      </MenuItem>
      <MenuItem icon={<PencilIcon />} onSelect={() => run(onRename)}>
        Rename…
      </MenuItem>
      <MenuSeparator />
      <MenuItem danger icon={<TrashIcon />} onSelect={() => run(onDelete)}>
        Delete folder
      </MenuItem>
    </>
  );
}

export function WorkflowFolderContextMenu({
  x,
  y,
  folderName,
  onClose,
  onCreateWorkflow,
  onRename,
  onDelete,
}: Props) {
  return (
    <ContextMenu x={x} y={y} onClose={onClose} animated zIndex={80}>
      <FolderMenuBody
        folderName={folderName}
        onCreateWorkflow={onCreateWorkflow}
        onRename={onRename}
        onDelete={onDelete}
      />
    </ContextMenu>
  );
}
