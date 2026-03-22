import { useNavigate } from "react-router-dom";
import { ArrowLeft } from "lucide-react";

export default function TermsPage() {
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
        <h1 className="font-headline font-extrabold text-4xl tracking-tight text-on-surface mb-2">Terms of Service</h1>
        <p className="text-on-surface-variant mb-12">Last updated: March 22, 2026</p>

        <div className="space-y-8 text-on-surface/90 leading-relaxed">
          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">1. Acceptance</h2>
            <p>
              By using Brivva ("the Service"), you agree to these Terms. If you do not agree, do not use the
              Service. Brivva is operated by Milliy Technology.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">2. Description of Service</h2>
            <p>
              Brivva provides real-time multilingual live commerce broadcasting. The Service translates a
              host's speech into multiple languages and distributes video streams to third-party platforms
              via RTMP. Brivva is not responsible for the availability or policies of third-party platforms
              (YouTube, Twitch, Instagram, TikTok, etc.).
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">3. Account and Access</h2>
            <ul className="list-disc pl-6 space-y-2">
              <li>You may connect third-party accounts (e.g., YouTube via Google OAuth) to use platform-specific features</li>
              <li>You are responsible for the security of your connected accounts and RTMP stream keys</li>
              <li>You must not share stream keys or credentials with unauthorized parties</li>
            </ul>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">4. Acceptable Use</h2>
            <p>You agree not to:</p>
            <ul className="list-disc pl-6 space-y-2 mt-2">
              <li>Use the Service for any unlawful purpose or to broadcast illegal content</li>
              <li>Violate the terms of service of any connected third-party platform</li>
              <li>Attempt to reverse-engineer, disrupt, or overload the Service</li>
              <li>Use the voice cloning feature to impersonate others without their consent</li>
            </ul>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">5. Content</h2>
            <p>
              You retain ownership of all content you broadcast through Brivva. You are solely responsible
              for the content of your streams, including translated output. Brivva does not review, endorse,
              or guarantee the accuracy of machine-translated content.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">6. Voice Cloning</h2>
            <p>
              By using the voice cloning feature, you confirm that you have the right to clone the voice
              provided (i.e., it is your own voice or you have explicit permission from the voice owner).
              Cloned voices are stored via ElevenLabs and can be deleted at any time from your dashboard.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">7. Third-Party Services</h2>
            <p>
              Brivva integrates with third-party services including Google/YouTube, Deepgram, and ElevenLabs.
              Your use of these services through Brivva is subject to their respective terms of service and
              privacy policies. Brivva is not liable for any changes, outages, or issues with third-party
              services.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">8. Limitation of Liability</h2>
            <p>
              The Service is provided "as is" without warranties of any kind. Brivva is not liable for any
              stream interruptions, translation errors, platform rejections, or data loss arising from the
              use of the Service. In no event shall Brivva's liability exceed the fees paid by you in the
              12 months preceding the claim.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">9. Changes to Terms</h2>
            <p>
              We may update these Terms at any time. Continued use of the Service after changes constitutes
              acceptance of the updated Terms. Material changes will be communicated via the Service or email.
            </p>
          </section>

          <section>
            <h2 className="font-headline font-bold text-xl text-on-surface mb-3">10. Contact</h2>
            <p>
              For questions about these Terms:{" "}
              <a href="mailto:legal@milliytechnology.org" className="text-primary hover:underline">
                legal@milliytechnology.org
              </a>
            </p>
          </section>
        </div>
      </main>
    </div>
  );
}
