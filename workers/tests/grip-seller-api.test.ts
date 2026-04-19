// Unit tests for the Grip Cloud Seller API wrapper.
//
// The exact endpoint path + response shape is still TODO — these tests
// pin the behavior we DO want:
//   1. happy path: extracts rtmpUrl + streamKey + broadcastId from the
//      expected response shape
//   2. auth-key validation fails fast (no network call) when keys missing
//   3. non-2xx HTTP → GripSellerApiError with the status propagated
//   4. 2xx with incomplete payload → 500 error
//
// When the real endpoint is pinned down, update the `match` regex and the
// body shape in the happy-path response; the rest of these cases should
// keep passing unchanged.

import { describe, it, expect, vi, afterEach } from "vitest";

import {
  provisionBroadcast,
  GripSellerApiError,
} from "../src/features/grip/seller-api";
import type { Env } from "../src/core/types";

const ENV = {} as unknown as Env;

afterEach(() => vi.unstubAllGlobals());

describe("provisionBroadcast (Grip Seller API)", () => {
  it("returns rtmpUrl + streamKey + broadcastId on happy path", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: "bcast_abc",
            ingest: {
              url: "rtmps://live.grip.fans:443/live/",
              stream_key: "sk_live_xxx",
            },
          }),
          { status: 200 },
        ),
      ),
    );

    const result = await provisionBroadcast(ENV, {
      accessKey: "ak",
      secretKey: "sk",
      productId: "prod_1",
      title: "hello",
    });

    expect(result).toEqual({
      rtmpUrl: "rtmps://live.grip.fans:443/live/",
      streamKey: "sk_live_xxx",
      broadcastId: "bcast_abc",
    });
  });

  it("throws 401 without hitting the network when keys are missing (sad)", async () => {
    const fetchFn = vi.fn();
    vi.stubGlobal("fetch", fetchFn);

    await expect(
      provisionBroadcast(ENV, {
        accessKey: "",
        secretKey: "",
        productId: "prod_1",
      }),
    ).rejects.toMatchObject({ status: 401 });
    expect(fetchFn).not.toHaveBeenCalled();
  });

  it("propagates non-2xx status as GripSellerApiError (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () => new Response("forbidden", { status: 403 }),
      ),
    );

    await expect(
      provisionBroadcast(ENV, {
        accessKey: "ak",
        secretKey: "sk",
        productId: "prod_1",
      }),
    ).rejects.toBeInstanceOf(GripSellerApiError);
  });

  it("returns 500 when response is missing expected fields (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ id: "bcast_no_ingest" }), {
            status: 200,
          }),
      ),
    );

    await expect(
      provisionBroadcast(ENV, {
        accessKey: "ak",
        secretKey: "sk",
        productId: "prod_1",
      }),
    ).rejects.toMatchObject({ status: 500 });
  });
});
