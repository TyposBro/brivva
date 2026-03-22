import { useNavigate } from "react-router-dom";
import { ArrowLeft } from "lucide-react";

export default function PrivacyPage() {
  const navigate = useNavigate();

  return (
    <div className="min-h-screen bg-background">
      <nav className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex items-center gap-4 max-w-3xl mx-auto px-8 h-20">
          <button onClick={() => navigate("/")} className="text-on-surface-variant hover:text-on-surface transition-colors">
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div className="text-2xl font-black tracking-tighter text-on-surface font-headline">BRIVVA</div>
        </div>
      </nav>

      <main className="max-w-3xl mx-auto px-8 pt-32 pb-20">
        <h1 className="font-headline font-extrabold text-4xl tracking-tight text-on-surface mb-2">Privacy Policy</h1>
        <p className="text-on-surface-variant mb-12">Last updated: March 22, 2026</p>

        <div className="space-y-8 text-on-surface/90 leading-relaxed">
          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">What Brivva Does</h2>
            <p>
              Brivva is a real-time multilingual live commerce broadcasting platform. Hosts connect their camera
              and microphone, and Brivva translates their speech into multiple languages and pushes video streams
              to platforms like YouTube, Twitch, and others via RTMP.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">Information We Collect</h2>
            <ul className="list-disc pl-6 space-y-2">
              <li>
                <strong>YouTube account data:</strong> When you connect your YouTube account via Google OAuth,
                we access your channel name, channel ID, and the ability to create and manage live broadcasts
                on your behalf. We request the <code className="text-sm bg-surface-container px-1.5 py-0.5 rounded">youtube</code> and <code className="text-sm bg-surface-container px-1.5 py-0.5 rounded">youtube.readonly</code> scopes.
              </li>
              <li>
                <strong>Audio and video:</strong> Your microphone audio and camera video are streamed to our
                server for real-time processing (speech-to-text, translation, text-to-speech) and RTMP
                distribution. We do not store recordings after the session ends.
              </li>
              <li>
                <strong>Voice samples:</strong> If you choose to clone your voice, a 30-second audio sample
                is sent to ElevenLabs for voice cloning. The cloned voice is stored in your account until you
                delete it.
              </li>
              <li>
                <strong>Session data:</strong> Session titles, stream configurations, and platform destinations
                are stored in our database to manage your broadcasts.
              </li>
            </ul>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">How We Use Your Data</h2>
            <ul className="list-disc pl-6 space-y-2">
              <li>To create and manage YouTube live broadcasts on your behalf</li>
              <li>To translate your speech and generate audio in other languages</li>
              <li>To push video streams to your configured RTMP destinations</li>
              <li>To clone your voice for use in translated streams (only if you opt in)</li>
            </ul>
            <p className="mt-3">We do not sell your data. We do not use your data for advertising.</p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">Third-Party Services</h2>
            <ul className="list-disc pl-6 space-y-2">
              <li><strong>Google/YouTube API:</strong> For OAuth authentication and broadcast management</li>
              <li><strong>Deepgram:</strong> For speech-to-text transcription</li>
              <li><strong>ElevenLabs:</strong> For text-to-speech and voice cloning</li>
            </ul>
            <p className="mt-3">
              Brivva's use and transfer of information received from Google APIs adheres to the{" "}
              <a
                href="https://developers.google.com/terms/api-services-user-data-policy"
                target="_blank"
                rel="noopener noreferrer"
                className="text-primary hover:underline"
              >
                Google API Services User Data Policy
              </a>
              , including the Limited Use requirements.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">Data Storage and Security</h2>
            <p>
              Session data and YouTube OAuth tokens are stored in an encrypted SQLite database. OAuth tokens
              are refreshed automatically and old tokens are overwritten. Audio and video data are processed
              in real-time and not persisted after the session ends.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">Data Deletion</h2>
            <p>
              You can delete your cloned voices at any time from the dashboard. To revoke YouTube access,
              visit your{" "}
              <a
                href="https://myaccount.google.com/permissions"
                target="_blank"
                rel="noopener noreferrer"
                className="text-primary hover:underline"
              >
                Google Account permissions
              </a>{" "}
              page. To request full account deletion, contact us at the email below.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">Contact</h2>
            <p>
              For privacy questions or data deletion requests:{" "}
              <a href="mailto:privacy@milliytechnology.org" className="text-primary hover:underline">
                privacy@milliytechnology.org
              </a>
            </p>
          </section>
        </div>
      </main>
    </div>
  );
}
