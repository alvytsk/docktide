import { Link, Outlet, useRouter } from "@tanstack/react-router";
import { useEffect } from "react";
import { Sidebar } from "./sidebar";

/** WebKitGTK leaves Back and Forward to browser chrome, which a Tauri window lacks. */
function useHistoryShortcuts() {
  const { history } = useRouter();
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.key === "ArrowLeft") history.back();
      else if (event.key === "ArrowRight") history.forward();
      else return;
      event.preventDefault();
    };
    // Mouse buttons 3 and 4 are the side buttons for Back and Forward.
    const onMouseUp = (event: MouseEvent) => {
      if (event.button === 3) history.back();
      else if (event.button === 4) history.forward();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [history]);
}

export function Shell() {
  useHistoryShortcuts();
  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <Outlet />
    </div>
  );
}

export function NotFound() {
  return (
    <main className="flex min-w-0 flex-1 flex-col items-start gap-3 px-10 py-8">
      <h1 className="text-xl font-semibold tracking-tight">Page not found</h1>
      <Link to="/containers" className="text-sm underline underline-offset-4">
        Back to containers
      </Link>
    </main>
  );
}
