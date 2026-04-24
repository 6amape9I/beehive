import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { useBootstrap } from "./BootstrapContext";
import { StatusBadge } from "../components/StatusBadge";

const navItems = [
  { to: "/dashboard", label: "Dashboard" },
  { to: "/entities", label: "Entities" },
  { to: "/entities/entity-0001", label: "Entity Detail" },
  { to: "/stages", label: "Stage Editor" },
  { to: "/workspace", label: "Workspace Explorer" },
  { to: "/settings", label: "Settings / Diagnostics" },
];

export function AppShell({ children }: { children: ReactNode }) {
  const { state, isBusy, reloadCurrentWorkdir } = useBootstrap();
  const projectName = state.project_name ?? "beehive";

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">bh</div>
          <div>
            <h1>{projectName}</h1>
            <p>Stage 1 foundation</p>
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
