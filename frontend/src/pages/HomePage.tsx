import { useState } from "react";
import { useNavigate } from "react-router-dom";

export default function HomePage() {
  const navigate = useNavigate();
  const [roomCode, setRoomCode] = useState("");

  const handleJoin = () => {
    const id = roomCode.trim().toUpperCase();
    if (id.length === 6) navigate(`/room/${id}`);
  };

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Real-time multilingual translation</p>
      </header>

      <main className="main">
        <button className="record-btn home-create-btn" onClick={() => navigate("/host")}>
          Create Room
        </button>

        <div className="home-divider"><span>or join a room</span></div>

        <div className="home-join">
          <input
            className="room-input"
            placeholder="Room code"
            value={roomCode}
            onChange={(e) => setRoomCode(e.target.value.toUpperCase())}
            onKeyDown={(e) => e.key === "Enter" && handleJoin()}
            maxLength={6}
            spellCheck={false}
          />
          <button
            className="record-btn home-join-btn"
            onClick={handleJoin}
            disabled={roomCode.trim().length !== 6}
          >
            Join
          </button>
        </div>
      </main>
    </div>
  );
}
