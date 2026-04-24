import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "./AppShell";
import { DashboardPage } from "../pages/DashboardPage";
import { EntitiesPage } from "../pages/EntitiesPage";
import { EntityDetailPage } from "../pages/EntityDetailPage";
import { SettingsDiagnosticsPage } from "../pages/SettingsDiagnosticsPage";
import { StageEditorPage } from "../pages/StageEditorPage";
import { WorkspaceExplorerPage } from "../pages/WorkspaceExplorerPage";

export function App() {
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<Navigate to="/dashboard" replace />} />
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
