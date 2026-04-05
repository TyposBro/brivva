type Props = {
  errors: string[];
  onDismiss: (index: number) => void;
};

export function ErrorBanners({ errors, onDismiss }: Props) {
  return (
    <>
      {errors.map((err, i) => (
        <div key={i} className="flex items-center justify-between bg-error-container text-on-error-container rounded-lg px-4 py-2 text-sm">
          <span>{err}</span>
          <button onClick={() => onDismiss(i)} className="ml-4 text-on-error-container/60 hover:text-on-error-container text-lg leading-none">x</button>
        </div>
      ))}
    </>
  );
}
