import type { ReactNode } from "react";

export function PageHeader({
  left,
  center,
  right,
}: {
  left?: ReactNode;
  center?: ReactNode;
  right?: ReactNode;
}) {
  return (
    <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
      <div className="flex justify-between items-center max-w-5xl mx-auto px-8 h-16">
        {left ?? <div />}
        {center ?? (
          <h1 className="text-xl font-bold tracking-tighter text-on-surface font-headline">
            BRIVVA
          </h1>
        )}
        {right ?? <div />}
      </div>
    </header>
  );
}
