type IconProps = { size?: number };

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
});

export const IconAdapter = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="3" y="9" width="18" height="11" rx="2" />
    <path d="M7 9V6a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v3" />
    <path d="M7 13h.01M11 13h.01M15 13h.01" />
  </svg>
);

export const IconSubnet = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="9" y="3" width="6" height="5" rx="1" />
    <rect x="2" y="16" width="6" height="5" rx="1" />
    <rect x="16" y="16" width="6" height="5" rx="1" />
    <path d="M12 8v4M12 12H5v4M12 12h7v4" />
  </svg>
);

export const IconDns = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3c2.5 2.5 4 5.7 4 9s-1.5 6.5-4 9c-2.5-2.5-4-5.7-4-9s1.5-6.5 4-9z" />
  </svg>
);

export const IconPing = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M2 12h4l3 8 4-16 3 8h6" />
  </svg>
);

export const IconAuto = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="2" y="6" width="20" height="12" rx="2" />
    <path d="M6 10h0M10 10h0M14 10h0M18 10h0M8 14h8" />
  </svg>
);

export const IconWifi = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M2 8.5a16 16 0 0 1 20 0" />
    <path d="M5 12a11 11 0 0 1 14 0" />
    <path d="M8.5 15.5a6 6 0 0 1 7 0" />
    <path d="M12 19h.01" />
  </svg>
);

export const IconAbout = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5M12 8h.01" />
  </svg>
);

export const IconCopy = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

export const IconGlobe = ({ size = 18 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3c2.5 2.5 4 5.7 4 9s-1.5 6.5-4 9c-2.5-2.5-4-5.7-4-9s1.5-6.5 4-9z" />
  </svg>
);
