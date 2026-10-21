import { Toast } from "@base-ui/react/toast";
import type { IpcError } from "@/bindings/IpcError";

/** Lets code outside React, such as mutation callbacks, show toasts. */
export const toastManager = Toast.createToastManager();

export function toastError(error: IpcError) {
  toastManager.add({
    type: "error",
    title: error.message,
    description: error.hint,
    priority: "high",
    timeout: 8000,
  });
}
