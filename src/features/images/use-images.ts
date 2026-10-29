import { useQuery } from "@tanstack/react-query";
import { createContext, use } from "react";
import type { ImageInspect } from "@/bindings/ImageInspect";
import { getBuildCacheUsage, getImage, listImages } from "@/lib/ipc";

export const imagesQueryKey = ["images"] as const;
export const imageQueryKey = (id: string) => ["image", id] as const;
export const buildCacheQueryKey = ["buildCache"] as const;

/** Disabled while disconnected; the connection panel explains why. */
export function useImages(enabled: boolean) {
  return useQuery({
    queryKey: imagesQueryKey,
    queryFn: listImages,
    enabled,
    // Errors carry a message and hint for the user; retrying would only delay them.
    retry: false,
  });
}

export function useImage(id: string, enabled: boolean) {
  return useQuery({
    queryKey: imageQueryKey(id),
    queryFn: () => getImage(id),
    enabled,
    retry: false,
  });
}

export function useBuildCache(enabled: boolean) {
  return useQuery({
    queryKey: buildCacheQueryKey,
    queryFn: getBuildCacheUsage,
    enabled,
    retry: false,
  });
}

/** The loaded image, provided by the details page to its tabs. */
export const ImageContext = createContext<ImageInspect | null>(null);

export function useLoadedImage(): ImageInspect {
  const image = use(ImageContext);
  if (!image) throw new Error("useLoadedImage outside a loaded image page");
  return image;
}
