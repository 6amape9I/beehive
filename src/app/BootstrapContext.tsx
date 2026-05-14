import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

import {
  initializeWorkdir as initializeWorkdirCommand,
  openWorkdir as openWorkdirCommand,
  openRegisteredWorkspace as openRegisteredWorkspaceCommand,
  reloadWorkdir as reloadWorkdirCommand,
} from "../lib/bootstrapApi";
import type { AppInitializationState } from "../types/domain";
import type { WorkspaceDescriptor } from "../types/domain";
import { notConfiguredState } from "../types/domain";

interface BootstrapContextValue {
  state: AppInitializationState;
  isBusy: boolean;
  lastActionError: string | null;
  initializeWorkdir: (path: string) => Promise<void>;
  openWorkdir: (path: string) => Promise<void>;
  openRegisteredWorkspace: (workspaceId: string) => Promise<void>;
  selectRegisteredWorkspace: (workspace: WorkspaceDescriptor) => void;
  reloadCurrentWorkdir: () => Promise<void>;
}

const BootstrapContext = createContext<BootstrapContextValue | null>(null);

export function BootstrapProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppInitializationState>(notConfiguredState);
  const [isBusy, setIsBusy] = useState(false);
  const [lastActionError, setLastActionError] = useState<string | null>(null);

  const runBootstrapAction = useCallback(
    async (action: () => Promise<{ state: AppInitializationState }>) => {
      setIsBusy(true);
      setLastActionError(null);
      try {
        const result = await action();
        setState(result.state);
      } catch (error: unknown) {
        const message = error instanceof Error ? error.message : String(error);
        setLastActionError(message);
      } finally {
        setIsBusy(false);
      }
    },
    [],
  );

  const initializeWorkdir = useCallback(
    async (path: string) => {
      await runBootstrapAction(() => initializeWorkdirCommand(path));
    },
    [runBootstrapAction],
  );

  const openWorkdir = useCallback(
    async (path: string) => {
      await runBootstrapAction(() => openWorkdirCommand(path));
    },
    [runBootstrapAction],
  );

  const openRegisteredWorkspace = useCallback(
    async (workspaceId: string) => {
      await runBootstrapAction(() => openRegisteredWorkspaceCommand(workspaceId));
    },
    [runBootstrapAction],
  );

  const selectRegisteredWorkspace = useCallback((workspace: WorkspaceDescriptor) => {
    setLastActionError(null);
    setState({
      ...notConfiguredState,
      phase: "fully_initialized",
      message: "Workspace selected for HTTP mode.",
      selected_workspace_id: workspace.id,
      selected_workdir_path: null,
      project_name: workspace.name,
      config_status: "server_managed",
      database_status: "server_managed",
    });
  }, []);

  const reloadCurrentWorkdir = useCallback(async () => {
    if (!state.selected_workdir_path) {
      setLastActionError("No workdir is selected.");
      return;
    }

    await runBootstrapAction(() => reloadWorkdirCommand(state.selected_workdir_path as string));
  }, [runBootstrapAction, state.selected_workdir_path]);

  const value = useMemo<BootstrapContextValue>(
    () => ({
      state,
      isBusy,
      lastActionError,
      initializeWorkdir,
      openWorkdir,
      openRegisteredWorkspace,
      selectRegisteredWorkspace,
      reloadCurrentWorkdir,
    }),
    [
      initializeWorkdir,
      isBusy,
      lastActionError,
      openRegisteredWorkspace,
      openWorkdir,
      reloadCurrentWorkdir,
      selectRegisteredWorkspace,
      state,
    ],
  );

  return <BootstrapContext.Provider value={value}>{children}</BootstrapContext.Provider>;
}

export function useBootstrap() {
  const context = useContext(BootstrapContext);
  if (!context) {
    throw new Error("useBootstrap must be used inside BootstrapProvider");
  }

  return context;
}
