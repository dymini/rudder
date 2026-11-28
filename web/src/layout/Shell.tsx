import { useQuery } from "@tanstack/react-query";
import { Suspense, useCallback, useEffect, useState } from "react";
import { Link, NavLink, Outlet, useLocation } from "react-router-dom";
import { api, setCsrf } from "../api";
import { useAuth } from "../auth";
import { Icon, DyminiPanelMark } from "../components/icons";
import {
  CommandPalette,
  ConnectionStatus,
  PaletteButton,
  usePaletteShortcut,
} from "../components/command-palette";
import { InsecureBanner } from "../components/insecure-banner";
import { LoadBanner, useLoad } from "../components/load";
import { Avatar, Skeleton } from "../components/ui";
import { msg, useT } from "../i18n";
import { setDarkTheme, useDarkTheme } from "../theme";
import type { Schemas } from "../types";
import { NAV_GROUPS } from "./nav";

const SIDEBAR_KEY = "dymini_panel_sidebar_collapsed";
const allItems = [...NAV_GROUPS.flatMap((group) => group.items.map((item) => ({ group, item })))];

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_KEY) === "1";
  } catch {
    return false;
  }
}

function currentPage(pathname: string) {
  if (pathname.startsWith("/services/")) {
    return { group: msg("Deploy"), label: msg("Service") };
  }
  if (pathname === "/account") {
    return { group: msg("You"), label: msg("Account") };
  }
  const match = allItems.find(({ item: [to] }) =>
    to === "/" ? pathname === "/" : pathname.startsWith(to),
  );
  return match ? { group: match.group.label, label: match.item[1] } : null;
}

interface NavBadges {
  firingAlerts: number;
  offlineServers: number;
  loadLevel: "ok" | "busy" | "critical";
}

function useNavBadges(): NavBadges {
  const alerts = useQuery({
    queryKey: ["alerts"],
    queryFn: () => api<Schemas["AlertView"][]>("/api/v1/alerts"),
    refetchInterval: 30_000,
    staleTime: 15_000,
  });
  const servers = useQuery({
    queryKey: ["servers"],
    queryFn: () => api<Schemas["ServerView"][]>("/api/v1/servers"),
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
  const load = useLoad();
  return {
    firingAlerts: (alerts.data ?? []).filter((r) => r.enabled && r.state === "firing").length,
    offlineServers: (servers.data ?? []).filter((s) => s.status === "offline").length,
    loadLevel: load.data?.guard.level ?? "ok",
  };
}

export function Shell() {
  const t = useT();
  const { pathname } = useLocation();
  const [open, setOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const [palette, setPalette] = useState(false);
  const badges = useNavBadges();
  const page = currentPage(pathname);
  const openPalette = useCallback(() => setPalette(true), []);
  const closePalette = useCallback(() => setPalette(false), []);
  usePaletteShortcut(openPalette);

  const toggleCollapsed = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      try {
        localStorage.setItem(SIDEBAR_KEY, next ? "1" : "0");
      } catch {
        // Ignore storage restrictions.
      }
      return next;
    });
  }, []);

  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  useEffect(() => {
    document.title = page ? `${t(page.label)} · Dymini Panel` : "Dymini Panel";
  }, [page, t]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const incidentCount =
    badges.firingAlerts + badges.offlineServers + (badges.loadLevel === "critical" ? 1 : 0);

  return (
    <div
      className={`min-h-screen transition-[grid-template-columns] duration-200 ${
        collapsed
          ? "lg:grid lg:grid-cols-[68px_minmax(0,1fr)]"
          : "lg:grid lg:grid-cols-[248px_minmax(0,1fr)]"
      }`}
    >
      <a
        href="#main"
        className="sr-only z-50 rounded-control bg-surface px-3 py-2 focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        {t("Skip to content")}
      </a>

      <div className="hidden border-r border-line bg-surface lg:block">
        <aside className="sticky top-0 h-screen">
          <Sidebar
            collapsed={collapsed}
            onToggleCollapse={toggleCollapsed}
            badges={badges}
          />
        </aside>
      </div>

      {open ? (
        <div className="fixed inset-0 z-40 lg:hidden">
          <button
            type="button"
            aria-label={t("Close menu")}
            className="absolute inset-0 bg-ink/30 backdrop-blur-[1px]"
            onClick={() => setOpen(false)}
          />
          <aside className="absolute inset-y-0 left-0 w-[280px] max-w-[85vw] animate-fade-up border-r border-line bg-surface shadow-overlay">
            <Sidebar onClose={() => setOpen(false)} badges={badges} />
          </aside>
        </div>
      ) : null}

      <div className="flex min-h-screen min-w-0 flex-col">
        <InsecureBanner />
        <LoadBanner />
        <header className="sticky top-0 z-30 flex h-14 items-center gap-3 border-b border-line bg-paper/85 px-4 backdrop-blur-md sm:px-6 lg:px-8">
          <button
            type="button"
            className="-ml-1 inline-flex size-9 items-center justify-center rounded-control text-ink-soft hover:bg-sunken lg:hidden"
            aria-label={t("Open menu")}
            aria-expanded={open}
            onClick={() => setOpen(true)}
          >
            <Icon name="menu" size={20} />
          </button>
          <Link
            to="/"
            className="flex items-center gap-2 lg:hidden"
            aria-label={t("Dymini Panel overview")}
          >
            <DyminiPanelMark size={24} />
            <span className="text-[15px] font-semibold tracking-[-0.01em]">Dymini Panel</span>
          </Link>
          {page ? (
            <nav
              aria-label={t("Breadcrumb")}
              className="hidden items-center gap-1.5 text-sm lg:flex"
            >
              <span className="text-muted">{t(page.group)}</span>
              <Icon name="chevronRight" size={14} className="text-faint" />
              <span className="font-medium text-ink">{t(page.label)}</span>
            </nav>
          ) : null}
          <div className="ml-auto flex items-center gap-1.5">
            {incidentCount > 0 && pathname !== "/" ? (
              <Link
                to={badges.firingAlerts > 0 ? "/alerts" : "/"}
                className="inline-flex h-8 items-center gap-1.5 rounded-full border border-danger-line bg-danger-soft px-2.5 text-xs font-medium text-danger transition-colors hover:bg-danger/15"
              >
                <span className="size-1.5 rounded-full bg-danger animate-live" aria-hidden="true" />
                <span className="hidden sm:inline">
                  {incidentCount === 1
                    ? t("{count} incident", { count: incidentCount })
                    : t("{count} incidents", { count: incidentCount })}
                </span>
                <span className="tabular-nums sm:hidden">{incidentCount}</span>
              </Link>
            ) : null}
            <PaletteButton onOpen={openPalette} />
            <ConnectionStatus />
            <ThemeToggle />
          </div>
        </header>
        <CommandPalette open={palette} onClose={closePalette} />
        <main
          id="main"
          className="mx-auto w-full max-w-[1440px] flex-1 space-y-6 px-4 py-5 sm:px-6 sm:py-6 lg:px-8"
        >
          <Suspense fallback={<PageSkeleton />}>
            <Outlet />
          </Suspense>
        </main>
      </div>
    </div>
  );
}

function Sidebar({
  collapsed = false,
  onToggleCollapse,
  onClose,
  badges,
}: {
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  onClose?: () => void;
  badges: NavBadges;
}) {
  const t = useT();
  return (
    <div className="flex h-full flex-col">
      <div
        className={`flex h-14 shrink-0 items-center border-b border-line ${
          collapsed ? "justify-center px-2" : "justify-between px-4"
        }`}
      >
        <Link
          to="/"
          className="flex items-center gap-2.5 overflow-hidden"
          aria-label={t("Dymini Panel overview")}
          title={collapsed ? "Dymini Panel" : undefined}
        >
          <DyminiPanelMark size={26} />
          {collapsed ? null : (
            <span className="truncate text-[15px] font-semibold tracking-[-0.01em] text-ink">
              Dymini Panel
            </span>
          )}
        </Link>
        {onClose ? (
          <button
            type="button"
            aria-label={t("Close menu")}
            className="inline-flex size-8 items-center justify-center rounded-control text-muted hover:bg-sunken hover:text-ink"
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        ) : onToggleCollapse && !collapsed ? (
          <button
            type="button"
            aria-label={t("Collapse sidebar")}
            title={t("Collapse sidebar")}
            className="inline-flex size-8 items-center justify-center rounded-control text-muted transition-colors hover:bg-sunken hover:text-ink"
            onClick={onToggleCollapse}
          >
            <Icon name="panelLeft" size={16} />
          </button>
        ) : null}
      </div>

      <nav
        aria-label={t("Main")}
        className={`flex-1 space-y-4 overflow-y-auto py-3 ${collapsed ? "px-2" : "px-3"}`}
      >
        {NAV_GROUPS.map((group) => (
          <div key={group.label}>
            {collapsed ? (
              <div className="mx-auto my-2 h-px w-6 bg-line" aria-hidden="true" />
            ) : (
              <p className="px-2.5 pb-1 text-[11px] font-semibold tracking-[0.04em] text-faint uppercase">
                {t(group.label)}
              </p>
            )}
            <ul className="space-y-0.5">
              {group.items.map(([to, label, icon]) => {
                const badge =
                  to === "/alerts" && badges.firingAlerts > 0
                    ? { count: badges.firingAlerts, tone: "danger" as const }
                    : to === "/servers" && badges.offlineServers > 0
                      ? { count: badges.offlineServers, tone: "danger" as const }
                      : to === "/load" && badges.loadLevel !== "ok"
                        ? {
                            dot: true,
                            tone: badges.loadLevel === "critical" ? ("danger" as const) : ("warning" as const),
                          }
                        : null;

                return (
                  <li key={to}>
                    <NavLink
                      to={to}
                      end={to === "/"}
                      title={collapsed ? t(label) : undefined}
                      className={({ isActive }) =>
                        `group relative flex h-9 items-center rounded-control text-sm transition-colors ${
                          collapsed ? "justify-center px-0" : "gap-2.5 px-2.5"
                        } ${
                          isActive
                            ? "bg-sunken font-medium text-ink"
                            : "text-ink-soft hover:bg-sunken/60 hover:text-ink"
                        }`
                      }
                    >
                      {({ isActive }) => (
                        <>
                          <Icon
                            name={icon}
                            className={
                              isActive ? "text-pine" : "text-faint group-hover:text-ink-soft"
                            }
                          />
                          {collapsed ? null : <span className="min-w-0 flex-1 truncate">{t(label)}</span>}
                          {badge ? (
                            collapsed ? (
                              <span
                                aria-hidden="true"
                                className={`absolute top-1.5 right-1.5 size-2 rounded-full ${
                                  badge.tone === "danger" ? "bg-danger" : "bg-amber"
                                }`}
                              />
                            ) : "count" in badge ? (
                              <span className="inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-danger-soft px-1.5 text-[11px] font-semibold text-danger tabular-nums">
                                {badge.count}
                              </span>
                            ) : (
                              <span
                                className={`size-2 rounded-full ${
                                  badge.tone === "danger" ? "bg-danger animate-live" : "bg-amber"
                                }`}
                              />
                            )
                          ) : null}
                        </>
                      )}
                    </NavLink>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </nav>

      {onToggleCollapse && collapsed ? (
        <div className="flex justify-center border-t border-line py-2">
          <button
            type="button"
            aria-label={t("Expand sidebar")}
            title={t("Expand sidebar")}
            className="inline-flex size-9 items-center justify-center rounded-control text-muted transition-colors hover:bg-sunken hover:text-ink"
            onClick={onToggleCollapse}
          >
            <Icon name="panelLeft" size={16} />
          </button>
        </div>
      ) : null}
      <UserCard collapsed={collapsed} />
    </div>
  );
}

function UserCard({ collapsed = false }: { collapsed?: boolean }) {
  const t = useT();
  const { user, refresh } = useAuth();
  const [pending, setPending] = useState(false);

  async function logout() {
    setPending(true);
    try {
      await api("/api/v1/logout", { method: "POST" });
      setCsrf(null);
      await refresh();
    } finally {
      setPending(false);
    }
  }

  const name = user?.display_name || user?.email || "";
  if (collapsed) {
    return (
      <div className="flex shrink-0 flex-col items-center gap-1 border-t border-line p-2">
        <NavLink
          to="/account"
          title={name}
          className={({ isActive }) =>
            `inline-flex size-9 items-center justify-center rounded-control transition-colors hover:bg-sunken ${isActive ? "bg-sunken" : ""}`
          }
        >
          <Avatar name={name} size={28} />
        </NavLink>
      </div>
    );
  }

  return (
    <div className="flex shrink-0 items-center gap-2 border-t border-line p-3">
      <NavLink
        to="/account"
        className={({ isActive }) =>
          `flex min-w-0 flex-1 items-center gap-2.5 rounded-control p-1.5 transition-colors hover:bg-sunken ${isActive ? "bg-sunken" : ""}`
        }
      >
        <Avatar name={name} size={32} />
        <span className="min-w-0 leading-tight">
          <span className="block truncate text-sm font-medium text-ink">{name}</span>
          <span className="block truncate text-xs text-muted first-letter:uppercase">
            {user?.role ? t(user.role) : null}
          </span>
        </span>
      </NavLink>
      <button
        type="button"
        aria-label={t("Sign out")}
        title={t("Sign out")}
        disabled={pending}
        onClick={() => void logout()}
        className="inline-flex size-9 shrink-0 items-center justify-center rounded-control text-muted transition-colors hover:bg-sunken hover:text-ink disabled:opacity-55"
      >
        <Icon name="logout" />
      </button>
    </div>
  );
}

function ThemeToggle() {
  const t = useT();
  const dark = useDarkTheme();
  const label = dark ? t("Use light theme") : t("Use dark theme");
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className="inline-flex size-9 items-center justify-center rounded-control text-ink-soft transition-colors hover:bg-sunken hover:text-ink"
      onClick={() => setDarkTheme(!dark)}
    >
      <Icon name={dark ? "sun" : "moon"} size={18} />
    </button>
  );
}

function PageSkeleton() {
  const t = useT();
  return (
    <div className="space-y-6" aria-busy="true" aria-label={t("Loading")}>
      <div className="space-y-2">
        <Skeleton className="h-7 w-48" />
        <Skeleton className="h-4 w-80 max-w-full" />
      </div>
      <Skeleton className="h-48 w-full rounded-card" />
      <Skeleton className="h-32 w-full rounded-card" />
    </div>
  );
}
