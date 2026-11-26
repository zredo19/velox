import { useId } from "react";
import { ANTIGRAVITY_PATH, ANTIGRAVITY_RAMPS } from "../../themes/agy/logo";
import type { AgentKind } from "../../types/ipc";

/* Official Claude mark. */
const CLAUDE_PATH =
  "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z";

const BLOCKED_COLOR = "#FF3B3B";

/* A soft halo behind the active Claude and Antigravity marks, so they read brighter. */
const GLOW = {
  claude: "rgba(240, 143, 105, 0.45)",
  agy: "rgba(120, 170, 255, 0.40)",
} as const;

const ACTIVE_COLORS: Record<AgentKind, string> = {
  claude: "#F08F69",
  agy: "#4285F4",
  codex: "#10A37F",
  opencode: "#C4B5FD",
  shell: "#94A3B8",
};

const MUTED_COLORS: Record<AgentKind, string> = {
  claude: "#A2604A",
  agy: "#3F6DB4",
  codex: "#166853",
  opencode: "#6E6591",
  shell: "#52607A",
};

/** The Antigravity arch painted like its CLI does: two vertical ramps blended left to right. */
function AntigravityMark({ className, active, blocked }: MarkProps) {
  // Ids are per instance: a gradient defined inside a hidden icon would not paint others.
  const id = useId().replace(/[^a-zA-Z0-9]/g, "");
  const stops = (colors: readonly string[]) =>
    colors.map((color, i) => (
      <stop key={color} offset={i / (colors.length - 1)} stopColor={color} />
    ));

  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      aria-hidden="true"
      style={
        blocked
          ? undefined
          : active
            ? { filter: `brightness(1.18) saturate(1.12) drop-shadow(0 0 3px ${GLOW.agy})` }
            : { opacity: 0.62, filter: "saturate(0.6)" }
      }
    >
      {blocked ? (
        <path d={ANTIGRAVITY_PATH} fill={BLOCKED_COLOR} />
      ) : (
        <>
          <defs>
            <linearGradient id={`${id}l`} x1="0" y1="0" x2="0" y2="1">
              {stops(ANTIGRAVITY_RAMPS.left)}
            </linearGradient>
            <linearGradient id={`${id}r`} x1="0" y1="0" x2="0" y2="1">
              {stops(ANTIGRAVITY_RAMPS.right)}
            </linearGradient>
            <linearGradient id={`${id}f`} x1="0" y1="0" x2="1" y2="0">
              <stop offset="0.36" stopColor="#000" />
              <stop offset="0.64" stopColor="#fff" />
            </linearGradient>
            <mask id={`${id}m`}>
              <rect width="24" height="24" fill={`url(#${id}f)`} />
            </mask>
          </defs>
          <path d={ANTIGRAVITY_PATH} fill={`url(#${id}l)`} />
          <path d={ANTIGRAVITY_PATH} fill={`url(#${id}r)`} mask={`url(#${id}m)`} />
        </>
      )}
    </svg>
  );
}

interface MarkProps {
  className: string;
  active: boolean;
  blocked: boolean;
}

export function AgentIcon({
  kind,
  className = "w-4 h-4",
  active = true,
  blocked = false,
}: {
  kind: AgentKind;
  className?: string;
  active?: boolean;
  /** The agent is waiting for an answer: painted red. */
  blocked?: boolean;
}) {
  const color = blocked ? BLOCKED_COLOR : active ? ACTIVE_COLORS[kind] : MUTED_COLORS[kind];

  switch (kind) {
    case "claude":
      return (
        <svg
          viewBox="0 0 24 24"
          className={className}
          aria-hidden="true"
          style={active && !blocked ? { filter: `drop-shadow(0 0 3px ${GLOW.claude})` } : undefined}
        >
          <path d={CLAUDE_PATH} fill={color} fillRule="evenodd" />
        </svg>
      );

    case "agy":
      return <AntigravityMark className={className} active={active} blocked={blocked} />;

    case "codex":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          className={className}
          aria-hidden="true"
        >
          <polygon
            points="12,2.7 20,7.3 20,16.7 12,21.3 4,16.7 4,7.3"
            stroke={color}
            strokeWidth="2"
            strokeLinejoin="round"
          />
          <path
            d="M9.5 9.5L7.5 12l2 2.5M14.5 9.5l2 2.5-2 2.5"
            stroke={color}
            strokeWidth="1.9"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      );

    case "opencode":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          className={className}
          aria-hidden="true"
        >
          <path
            d="M12 3l8.5 9L12 21 3.5 12 12 3z"
            stroke={color}
            strokeWidth="2"
            strokeLinejoin="round"
          />
          <path
            d="M12 7.5l4.2 4.5-4.2 4.5L7.8 12 12 7.5z"
            fill={color}
            fillOpacity={active ? "0.28" : "0.14"}
          />
        </svg>
      );

    case "shell":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          className={className}
          aria-hidden="true"
        >
          <path
            d="M5 7.5L10.5 12 5 16.5"
            stroke={color}
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            d="M12.5 17H19"
            stroke={color}
            strokeWidth="2.2"
            strokeLinecap="round"
          />
        </svg>
      );
  }
}

/**
 * Velox mark: a terminal window running several agents at once. The prompt chevron leads
 * three output lanes, one per agent colour (Claude, Antigravity, Codex), with the cursor
 * blinking at the end of the shortest one.
 */
export function VeloxLogo({ className = "w-4 h-4" }: { className?: string }) {
  const id = useId().replace(/[^a-zA-Z0-9]/g, "");
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <defs>
        <linearGradient id={`${id}bg`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#1B2233" />
          <stop offset="1" stopColor="#0C1019" />
        </linearGradient>
        <linearGradient id={`${id}rim`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#F08F69" />
          <stop offset="0.5" stopColor="#7AA8FF" />
          <stop offset="1" stopColor="#34D3A0" />
        </linearGradient>
      </defs>
      <rect x="1.25" y="1.25" width="21.5" height="21.5" rx="5.5" fill={`url(#${id}bg)`} />
      <rect
        x="1.25"
        y="1.25"
        width="21.5"
        height="21.5"
        rx="5.5"
        stroke={`url(#${id}rim)`}
        strokeOpacity="0.75"
        strokeWidth="1.1"
      />
      <path
        d="M5.6 8.2L8.9 12L5.6 15.8"
        stroke="#F2F5FB"
        strokeWidth="2.1"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="M11.6 8.2H18.4" stroke="#F08F69" strokeWidth="2.1" strokeLinecap="round" />
      <path d="M11.6 12H16.6" stroke="#7AA8FF" strokeWidth="2.1" strokeLinecap="round" />
      <path d="M11.6 15.8H13.6" stroke="#34D3A0" strokeWidth="2.1" strokeLinecap="round" />
      <rect x="15.2" y="14.6" width="2.3" height="2.4" rx="0.5" fill="#F2F5FB" fillOpacity="0.85" />
    </svg>
  );
}
