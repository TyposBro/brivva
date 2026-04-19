import { BrowserRouter, Routes, Route } from "react-router-dom";
import HomePage from "../features/public/presentation/home-page";
import HostPage from "../features/broadcast/presentation/host-page";
import DashboardPage from "../features/broadcast/presentation/dashboard-page";
import SessionPage from "../features/broadcast/presentation/session-page";
import PrivacyPage from "../features/public/presentation/privacy-page";
import TermsPage from "../features/public/presentation/terms-page";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/host" element={<HostPage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="/session/:id" element={<SessionPage />} />
        <Route path="/privacy" element={<PrivacyPage />} />
        <Route path="/terms" element={<TermsPage />} />
      </Routes>
    </BrowserRouter>
  );
}
