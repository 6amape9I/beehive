import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { useBootstrap } from "./BootstrapContext";
import { StatusBadge } from "../components/StatusBadge";

export function AppShell({ children }: { children: ReactNode }) {
  const { state, isBusy, reloadCurrentWorkdir } = useBootstrap();
  const projectName = state.project_name ?? state.selected_workspace_id ?? "beehive";
  const workspaceBase = state.selected_workspace_id
    ? `/workspaces/${encodeURIComponent(state.selected_workspace_id)}`
    : null;
  const navItems = [
    { to: "/workspaces", label: "Workspaces" },
    { to: workspaceBase ? `${workspaceBase}/workspace` : "/workspace", label: "Workspace Explorer" },
    { to: workspaceBase ? `${workspaceBase}/stages` : "/stages", label: "Stage Editor" },
    { to: workspaceBase ? `${workspaceBase}/entities` : "/entities", label: "Entities" },
    { to: "/dashboard", label: "Dashboard" },
    { to: "/settings", label: "Settings / Diagnostics" },
  ];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">bh</div>
          <div>
            <h1>{projectName}</h1>
            <p>Stage 2 runtime foundation</p>
          </div>
        </div>
        <nav className="nav-list" aria-label="Main navigation">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) => (isActive ? "nav-link active" : "nav-link")}
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
      </aside>

      <div className="main-column">
        <header className="topbar">
          <div>
            <span className="eyebrow">Current state</span>
            <div className="topbar-title">
              <StatusBadge status={state.phase} />
              <span>{state.message}</span>
              {state.selected_workspace_id ? <span>Workspace {state.selected_workspace_id}</span> : null}
            </div>
          </div>
          <button
            type="button"
            className="button secondary"
            onClick={() => void reloadCurrentWorkdir()}
            disabled={isBusy || !state.selected_workdir_path}
          >
            Reload
          </button>
        </header>

        <main className="content">{children}</main>
      </div>
    </div>
  );
}
