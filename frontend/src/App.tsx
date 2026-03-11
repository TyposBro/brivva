import { BrowserRouter, Routes, Route } from "react-router-dom";
import HomePage from "./pages/HomePage";
import HostPage from "./pages/HostPage";
import GuestPage from "./pages/GuestPage";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/host" element={<HostPage />} />
        <Route path="/room/:id" element={<GuestPage />} />
      </Routes>
    </BrowserRouter>
  );
}
