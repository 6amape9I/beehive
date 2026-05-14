import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "./AppShell";
import { DashboardPage } from "../pages/DashboardPage";
import { EntitiesPage } from "../pages/EntitiesPage";
import { EntityDetailPage } from "../pages/EntityDetailPage";
import { SettingsDiagnosticsPage } from "../pages/SettingsDiagnosticsPage";
import { StageEditorPage } from "../pages/StageEditorPage";
import { WorkspaceExplorerPage } from "../pages/WorkspaceExplorerPage";
import { WorkspaceSelectorPage } from "../pages/WorkspaceSelectorPage";

export function App() {
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<Navigate to="/workspaces" replace />} />
        <Route path="/workspaces" element={<WorkspaceSelectorPage />} />
        <Route path="/workspaces/:workspaceId/workspace" element={<WorkspaceExplorerPage />} />
        <Route path="/workspaces/:workspaceId/stages" element={<StageEditorPage />} />
        <Route path="/workspaces/:workspaceId/entities" element={<EntitiesPage />} />
        <Route path="/workspaces/:workspaceId/entities/:entityId" element={<EntityDetailPage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="/entities" element={<EntitiesPage />} />
        <Route path="/entities/:entityId" element={<EntityDetailPage />} />
        <Route path="/stages" element={<StageEditorPage />} />
        <Route path="/workspace" element={<WorkspaceExplorerPage />} />
        <Route path="/settings" element={<SettingsDiagnosticsPage />} />
      </Routes>
    </AppShell>
  );
}
