import { useNavigate } from "react-router-dom";

export default function HomePage() {
  const navigate = useNavigate();

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Real-time multilingual live commerce</p>
      </header>

      <main className="main">
        <button className="record-btn home-create-btn" onClick={() => navigate("/dashboard")}>
          Stream Dashboard
        </button>
      </main>
    </div>
  );
}
