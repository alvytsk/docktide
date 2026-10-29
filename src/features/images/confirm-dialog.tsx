import { useState, type ReactNode } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";

type Props<T> = {
  trigger: string;
  disabled?: boolean;
  title: (frozen: T) => string;
  confirm: string;
  /** Live value; copied when the dialog opens so the dialog and onConfirm see one fixed value. */
  source: T;
  onConfirm: (frozen: T) => void;
  children: (frozen: T) => ReactNode;
};

/** A destructive action behind a confirm step; the trigger is an outline button. */
export function ConfirmDialog<T>({
  trigger,
  disabled,
  title,
  confirm,
  source,
  onConfirm,
  children,
}: Props<T>) {
  const [open, setOpen] = useState(false);
  const [frozen, setFrozen] = useState(source);
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (next) setFrozen(source);
        setOpen(next);
      }}
    >
      <AlertDialogTrigger
        disabled={disabled}
        render={<Button variant="outline" size="sm" disabled={disabled} />}
      >
        {trigger}
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title(frozen)}</AlertDialogTitle>
          <AlertDialogDescription render={<div />}>{children(frozen)}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            onClick={() => {
              setOpen(false);
              onConfirm(frozen);
            }}
          >
            {confirm}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
