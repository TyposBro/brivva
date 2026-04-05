import { useState, useEffect } from "react";

export function useMediaDevices() {
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);

  useEffect(() => {
    enumerateDevices().then(setDevices);
  }, []);

  return devices;
}

async function enumerateDevices(): Promise<MediaDeviceInfo[]> {
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
    stream.getTracks().forEach((t) => t.stop());
  } catch {
    // Permission denied — enumerate what's available without labels
  }
  return navigator.mediaDevices.enumerateDevices();
}
