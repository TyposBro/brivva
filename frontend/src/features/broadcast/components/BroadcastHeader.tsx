type Props = { sessionId: string | null };

export function BroadcastHeader({ sessionId }: Props) {
  return (
    <div className="flex items-center justify-between">
      <h1 className="font-headline text-2xl font-bold text-primary">Brivva</h1>
      {sessionId && <span className="text-xs font-mono text-outline">Session: {sessionId}</span>}
    </div>
  );
}
