import { type Lang, LANGS, LANG_LABELS } from "../../../types";

type Props = {
  roomId: string;
  onSelectLang: (lang: Lang) => void;
};

export function LanguagePicker({ roomId, onSelectLang }: Props) {
  const handleSelect = (lang: Lang) => {
    unlockBrowserAudio();
    onSelectLang(lang);
  };

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Room {roomId}</p>
      </header>
      <main className="main">
        <p className="lang-pick-label">Choose your language</p>
        <div className="lang-picker">
          {LANGS.map((lang) => (
            <button key={lang} className="lang-pick-btn" onClick={() => handleSelect(lang)}>
              {LANG_LABELS[lang]}
            </button>
          ))}
        </div>
      </main>
    </div>
  );
}

function unlockBrowserAudio() {
  const ctx = new AudioContext();
  ctx.resume().then(() => ctx.close());
}
