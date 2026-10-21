import { Providers } from "@/app/providers";
import { Sidebar } from "@/app/sidebar";
import { ContainersPage } from "@/features/containers/containers-page";

export function App() {
  return (
    <Providers>
      <div className="flex h-screen overflow-hidden">
        <Sidebar />
        <ContainersPage />
      </div>
    </Providers>
  );
}
