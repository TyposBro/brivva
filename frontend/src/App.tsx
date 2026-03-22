import { BrowserRouter, Routes, Route } from "react-router-dom";
import HomePage from "./pages/HomePage";
import HostPage from "./pages/HostPage";
import DashboardPage from "./pages/DashboardPage";
import SessionPage from "./pages/SessionPage";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/host" element={<HostPage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="/session/:id" element={<SessionPage />} />
      </Routes>
    </BrowserRouter>
  );
}
