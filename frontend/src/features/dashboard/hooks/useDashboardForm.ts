import { useState } from "react";

export function useDashboardForm() {
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [privacyStatus, setPrivacyStatus] = useState("unlisted");
  const [showSettings, setShowSettings] = useState(false);

  return { title, sourceLang, privacyStatus, showSettings, setTitle, setSourceLang, setPrivacyStatus, setShowSettings };
}
