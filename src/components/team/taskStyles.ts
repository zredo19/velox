import type { SessionStatus, TaskState } from "../../types/ipc";

export function getTaskStateBadge(state: TaskState): {
  label: string;
  className: string;
} {
  switch (state) {
    case "pending":
      return {
        label: "Pending",
        className: "bg-amber-500/15 text-amber-300 border-amber-500/25",
      };
    case "accepted":
      return {
        label: "Accepted",
        className: "bg-sky-500/15 text-sky-300 border-sky-500/25",
      };
    case "delivered":
      return {
        label: "Delivered",
        className: "bg-emerald-500/15 text-emerald-300 border-emerald-500/25",
      };
    case "rejected":
      return {
        label: "Rejected",
        className: "bg-rose-500/15 text-rose-300 border-rose-500/25",
      };
    case "failed":
      return {
        label: "Failed",
        className: "bg-rose-500/15 text-rose-300 border-rose-500/25",
      };
    case "expired":
      return {
        label: "Expired",
        className: "bg-slate-500/15 text-slate-300 border-slate-500/25",
      };
  }
}

export function formatTaskTime(iso: string): string {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return "";
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function getSessionStatusDotClass(status: SessionStatus): string {
  switch (status) {
    case "starting":
    case "running":
      return "bg-emerald-400";
    case "idle":
      return "bg-[#98A2B8]";
    case "blocked":
      return "bg-[#F43F5E]";
    case "sleeping":
    case "exited":
      return "bg-[#52607A]";
  }
}
