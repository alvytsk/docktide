// The only module that calls invoke(). Rejections are normalized to IpcError.
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import type { ConnectionStatus } from "@/bindings/ConnectionStatus";
import type { IpcError } from "@/bindings/IpcError";
import { toIpcError } from "./errors";

declare module "@tanstack/react-query" {
  interface Register {
    defaultError: IpcError;
  }
}

async function call<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toIpcError(error);
  }
}

export const getConnectionStatus = () => call<ConnectionStatus>("get_connection_status");
