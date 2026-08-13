import { create } from "zustand";
import type {
  AgentAuthRequired,
  AgentProviderId,
} from "../../features/workflow/types";

export type AgentAuthToast = {
  id: string;
  provider: AgentProviderId;
  label: string;
  loginCommand: string;
  workflowName?: string;
};

type ToastStore = {
  toasts: AgentAuthToast[];
  showAgentAuthToast: (
    authRequired: AgentAuthRequired,
    workflowName?: string,
  ) => void;
  dismissToast: (id: string) => void;
};

export const useToastStore = create<ToastStore>((set) => ({
  toasts: [],
  showAgentAuthToast: (authRequired, workflowName) =>
    set((state) => {
      const toast: AgentAuthToast = {
        id: `agent-auth:${authRequired.provider}`,
        provider: authRequired.provider,
        label: authRequired.label,
        loginCommand: authRequired.loginCommand,
        workflowName,
      };
      const existingIndex = state.toasts.findIndex(
        (item) => item.id === toast.id,
      );

      if (existingIndex === -1) {
        return { toasts: [...state.toasts, toast] };
      }

      const toasts = [...state.toasts];
      toasts[existingIndex] = toast;
      return { toasts };
    }),
  dismissToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),
}));
