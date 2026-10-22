import { RouterProvider } from "@tanstack/react-router";
import { Providers } from "@/app/providers";
import { createAppRouter } from "@/app/router";

const router = createAppRouter();

export function App() {
  return (
    <Providers>
      <RouterProvider router={router} />
    </Providers>
  );
}
