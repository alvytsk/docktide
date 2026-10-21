import type { ErrorKind } from "@/bindings/ErrorKind";
import type { IpcError } from "@/bindings/IpcError";

const KIND_LABELS: Record<ErrorKind, string> = {
  daemonUnavailable: "Docker not running",
  permissionDenied: "Permission denied",
  unsupportedEndpoint: "Unsupported endpoint",
  invalidConfiguration: "Invalid configuration",
  apiVersionUnsupported: "Engine too old",
  notFound: "Not found",
  conflict: "Conflict",
  timeout: "No response",
  internal: "Error",
};

function isIpcError(error: unknown): error is IpcError {
  return (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    typeof error.kind === "string" &&
    Object.hasOwn(KIND_LABELS, error.kind) &&
    "message" in error &&
    typeof error.message === "string"
  );
}

/** Normalizes an `invoke()` rejection. Tauri itself rejects with plain strings. */
export function toIpcError(error: unknown): IpcError {
  if (isIpcError(error)) return error;
  if (error instanceof Error) return { kind: "internal", message: error.message };
  return { kind: "internal", message: String(error) };
}

/** A short name for the error kind, for compact places like the sidebar indicator. */
export function errorKindLabel(kind: ErrorKind): string {
  return KIND_LABELS[kind];
}
