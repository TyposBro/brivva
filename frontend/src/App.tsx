import { BrowserRouter, Routes, Route } from "react-router-dom";
import HomePage from "./pages/HomePage";
import HostPage from "./features/broadcast/presentation/host-page";
import DashboardPage from "./features/broadcast/presentation/dashboard-page";
import SessionPage from "./features/broadcast/presentation/session-page";
import PrivacyPage from "./pages/PrivacyPage";
import TermsPage from "./pages/TermsPage";

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
