import { Mic, SkipForward, StopCircle } from "lucide-react";
import { cn } from "../../../core/cn";
import type { SourceLang } from "./source-lang-picker";

interface VoiceSetupCardProps {
  elapsedSec: number;
  isRecording: boolean;
  minSec: number;
  maxSec: number;
  onStart: () => void;
  onStop: () => void;
  onSkip: () => void;
  /** Language the host will speak on stream. Drives which sample script is
   *  shown so the voice clone is enrolled in the same language as live audio. */
  sourceLang: SourceLang;
}

/** Live-commerce host monologues, ~60-90s at natural pace. Each one mixes
 *  welcomes, product tease, engagement prompt, and close so we cover a wide
 *  phoneme range for the voice clone. Keep these native — no romanization. */
const SAMPLE_SCRIPTS: Record<SourceLang, string> = {
  en:
    "Hey everyone, welcome back to the show! I'm so excited you're here with " +
    "me tonight. The quick brown fox jumps over the lazy dog, and honestly, " +
    "that's how fast these pieces have been flying off the shelf. Let me " +
    "walk you through what I've lined up: a buttery-soft knit, a weekend " +
    "jacket, and a zesty little fragrance that my friends keep stealing from " +
    "my bag. The texture, the stitching, the color payoff — all of it feels " +
    "way more premium than the price tag suggests. Drop a heart in the chat " +
    "if you're watching live, tell me where you're tuning in from, and if " +
    "you've got questions about sizing, shipping, or the bundle discount, " +
    "shoot them over and I'll answer every single one. Stay with me, because " +
    "the first fifty orders get a surprise freebie, and trust me, you do " +
    "not want to miss this drop.",
  ko:
    "여러분 안녕하세요! 오늘도 찾아와 주셔서 정말 감사드려요. 오늘은 특별히 " +
    "준비한 제품들이 많으니까 끝까지 함께해 주세요. 이 제품은 제가 한 달 " +
    "동안 직접 써보고 자신 있게 소개해 드리는 건데요, 촉감이 정말 부드럽고 " +
    "발색도 아주 자연스러워요. 색상은 총 다섯 가지로 준비되어 있고, 사이즈 " +
    "고민 있으신 분들은 채팅창에 키, 몸무게 남겨 주시면 제가 바로바로 " +
    "추천해 드릴게요. 지금 구매하시는 분들께는 배송비 무료에 사은품까지 " +
    "챙겨드리니까, 망설이지 마시고 빠르게 주문 눌러 주세요. 재고가 많지 " +
    "않아서 조금 늦으면 품절될 수 있어요. 궁금한 점 있으시면 언제든 " +
    "말씀해 주시고, 좋아요와 하트도 꾹 눌러 주시면 정말 힘이 납니다. " +
    "자, 그럼 본격적으로 시작해 볼까요?",
  ja:
    "みなさん、こんばんは!今日も遊びに来てくださって本当にありがとう" +
    "ございます。今夜は特別なアイテムをたっぷりご用意しましたので、" +
    "ぜひ最後までお付き合いくださいね。こちらの商品、実は私も毎日" +
    "愛用していて、肌触りがとても柔らかくて、着心地も抜群なんです。" +
    "カラーバリエーションは全部で六色、サイズはSからXLまで揃って" +
    "います。今ご注文いただいた方には、送料無料に加えて、限定の" +
    "ミニポーチをプレゼント中です。コメント欄にお住まいの地域と" +
    "サイズの悩みを書き込んでくだされば、私がその場でおすすめを" +
    "お伝えしますよ。在庫が残りわずかなので、気になる方はお早めに" +
    "どうぞ。ハートマークやいいねもたくさん押してくださると、" +
    "とっても励みになります。それでは、盛り上がっていきましょう!",
  zh:
    "大家晚上好,欢迎来到今天的直播间!能看到这么多熟悉的朋友,我真的" +
    "太开心啦。今天我给大家准备了好几款超值单品,保证看完你们都想马上" +
    "下单。先说这款,我自己已经用了整整一个月,质地细腻,上脸特别服帖," +
    "颜色也非常日常,不管通勤还是约会都百搭。尺码方面大家不用担心," +
    "从小码到加大码全都有货,把你的身高体重打在公屏上,我立刻给你推荐" +
    "最合适的。现在下单不仅包邮到家,还额外赠送一份小礼物,数量不多, " +
    "先到先得。有任何问题,比如材质、洗涤、到货时间,都可以直接在评论区" +
    "留言,我会一条一条认真回复。记得点亮小红心,分享给你们的好姐妹, " +
    "咱们马上开抢!",
};

function fmtMmSs(sec: number): string {
  const m = Math.floor(sec / 60).toString().padStart(2, "0");
  const s = (sec % 60).toString().padStart(2, "0");
  return `${m}:${s}`;
}

export function VoiceSetupCard({
  elapsedSec,
  isRecording,
  minSec,
  maxSec,
  onStart,
  onStop,
  onSkip,
  sourceLang,
}: VoiceSetupCardProps) {
  const minReached = elapsedSec >= minSec;
  const denom = minReached ? maxSec : minSec;
  const pct = Math.min(100, Math.round((elapsedSec / denom) * 100));
  const label = minReached
    ? `${fmtMmSs(elapsedSec)} / ${fmtMmSs(maxSec)} max`
    : `${fmtMmSs(elapsedSec)} / ${fmtMmSs(minSec)} (minimum)`;
  const script = SAMPLE_SCRIPTS[sourceLang] ?? SAMPLE_SCRIPTS.en;

  return (
    <section className="bg-surface-container-low rounded-xl p-8 max-w-lg mx-auto space-y-6">
      <div>
        <h3 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Voice Setup
        </h3>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          Record at least {minSec} seconds. Longer samples (up to{" "}
          {Math.floor(maxSec / 60)} minutes) produce a noticeably better clone
          and reduce accent bleed in the translated voice.
        </p>
      </div>

      {!isRecording && elapsedSec === 0 && (
        <button
          className="monolith-gradient text-white w-full py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all flex items-center justify-center gap-2"
          onClick={onStart}
        >
          <Mic className="w-5 h-5" />
          Record Voice Sample
        </button>
      )}

      {isRecording && (
        <>
          <div className="flex items-center gap-3 text-error font-label">
            <span className="w-2 h-2 bg-error rounded-full animate-pulse" />
            Recording…
            <span className="ml-auto text-on-surface-variant text-xs tabular-nums">
              {label}
            </span>
          </div>

          <div className="h-2 w-full bg-surface-container-highest rounded-full overflow-hidden">
            <div
              className={cn(
                "h-full transition-all rounded-full",
                minReached ? "bg-success" : "bg-primary",
              )}
              style={{ width: `${pct}%` }}
            />
          </div>

          <div
            lang={sourceLang}
            className="bg-surface-container-highest rounded-lg p-4 text-on-surface-variant text-sm leading-relaxed font-body italic break-words"
          >
            {script}
          </div>

          <button
            className={cn(
              "w-full py-3 rounded-xl font-headline font-bold flex items-center justify-center gap-2 transition-all",
              minReached
                ? "bg-success text-white hover:scale-[0.98]"
                : "bg-surface-container-high text-on-surface-variant cursor-not-allowed",
            )}
            disabled={!minReached}
            onClick={onStop}
            title={
              minReached ? "Stop recording and clone voice" : `Wait ${minSec - elapsedSec}s`
            }
          >
            <StopCircle className="w-5 h-5" />
            {minReached ? "Stop & Clone" : `${minSec - elapsedSec}s to minimum`}
          </button>
        </>
      )}

      <button
        className="w-full bg-surface-container-high hover:bg-surface-bright text-on-surface-variant py-2.5 rounded-lg font-label text-sm transition-colors flex items-center justify-center gap-2"
        onClick={onSkip}
      >
        <SkipForward className="w-4 h-4" />
        Skip (use default voice)
      </button>
    </section>
  );
}
