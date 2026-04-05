import type { ReactNode } from "react";

const MAX_WIDTH_MAP = {
  "2xl": "max-w-2xl",
  "3xl": "max-w-3xl",
  "5xl": "max-w-5xl",
  "7xl": "max-w-7xl",
} as const;

const PADDING_MAP = {
  "2xl": "px-6",
  "3xl": "px-8",
  "5xl": "px-8",
  "7xl": "px-8",
} as const;

const SPACE_MAP = {
  6: "space-y-6",
  8: "space-y-8",
  12: "space-y-12",
} as const;

type MaxWidth = keyof typeof MAX_WIDTH_MAP;
type SpaceY = keyof typeof SPACE_MAP;

type Props = {
  left?: ReactNode;
  right?: ReactNode;
  maxWidth?: MaxWidth;
  spaceY?: SpaceY;
  onTitleClick?: () => void;
  children: ReactNode;
};

export function PageLayout({
  left,
  right,
  maxWidth = "5xl",
  spaceY = 8,
  onTitleClick,
  children,
}: Props) {
  const widthClass = MAX_WIDTH_MAP[maxWidth];
  const paddingClass = PADDING_MAP[maxWidth];
  const spaceClass = SPACE_MAP[spaceY];

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className={`flex justify-between items-center ${widthClass} mx-auto ${paddingClass} h-16`}>
          {left ?? <div />}
          <h1
            className={`text-xl font-bold tracking-tighter text-on-surface font-headline${onTitleClick ? " cursor-pointer" : ""}`}
            onClick={onTitleClick}
          >
            BRIVVA
          </h1>
          {right ?? <div />}
        </div>
      </header>

      <main className={`${widthClass} mx-auto ${paddingClass} pt-24 pb-16 ${spaceClass}`}>
        {children}
      </main>
    </div>
  );
}
