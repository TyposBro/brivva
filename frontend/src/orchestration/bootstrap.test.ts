import { describe, expect, it } from "vitest";
import { toWebSocketBase } from "./bootstrap";

describe("bootstrap media URL config", () => {
	it("converts http(s) media engine URLs to ws(s)", () => {
		expect(toWebSocketBase("https://engine.example.com")).toBe(
			"wss://engine.example.com",
		);
		expect(toWebSocketBase("http://localhost:3000")).toBe(
			"ws://localhost:3000",
		);
	});

	it("leaves explicit websocket URLs unchanged", () => {
		expect(toWebSocketBase("wss://engine.example.com")).toBe(
			"wss://engine.example.com",
		);
		expect(toWebSocketBase("ws://localhost:3000")).toBe("ws://localhost:3000");
	});
});
