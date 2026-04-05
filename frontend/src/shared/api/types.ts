export type UserInfo = {
  id: string;
  youtube_connected: boolean;
  youtube_channel_name: string | null;
  youtube_channel_id: string | null;
  created_at: number;
};

export type Session = {
  id: string;
  user_id: string;
  voice_id: string | null;
  title: string;
  source_lang: string;
  target_langs: string;
  status: string;
  room_id: string | null;
  created_at: number;
};

export type StreamInfo = {
  id: string;
  lang: string;
  platform?: string;
  broadcast_id?: string;
  stream_id?: string;
  rtmp_url?: string;
  stream_key?: string;
  status?: string;
  error?: string;
};

export type CreateSessionResponse = {
  session: Session;
  streams: StreamInfo[];
  errors?: string[];
};

export type PlatformConfig = {
  platform: string;
  lang?: string;
  rtmp_url?: string;
  stream_key?: string;
};

export type Voice = {
  id: string;
  user_id: string;
  elevenlabs_voice_id: string;
  name: string;
  created_at: number;
};

export type PlatformCredential = {
  id: string;
  user_id: string;
  platform: string;
  rtmp_url: string | null;
  stream_key: string | null;
  display_name: string | null;
  created_at: number;
  updated_at: number;
};
