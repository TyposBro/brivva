// JWT issuance / verification.
//
// Workers signs short-lived HS256 tokens; Fargate verifies them with the same
// JWT_SECRET (shared via AWS Secrets Manager + Workers secret). Fargate's
// `jsonwebtoken` crate produces identical tokens for the same claims + secret.

import { SignJWT, jwtVerify } from "jose";

export type JWTClaims = {
  sub: string; // user_id
  voice_clone_id?: string;
  session_id?: string;
  // jose handles iat/exp
};

const encoder = new TextEncoder();

const ISSUER = "brivva-api";
const AUDIENCE = "brivva-fargate";
const TTL_SECONDS = 60 * 15; // 15 min

export async function signJwt(secret: string, claims: JWTClaims): Promise<string> {
  const key = encoder.encode(secret);
  return await new SignJWT({ ...claims })
    .setProtectedHeader({ alg: "HS256", typ: "JWT" })
    .setIssuer(ISSUER)
    .setAudience(AUDIENCE)
    .setSubject(claims.sub)
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + TTL_SECONDS)
    .sign(key);
}

export async function verifyJwt(secret: string, token: string): Promise<JWTClaims> {
  const key = encoder.encode(secret);
  const { payload } = await jwtVerify(token, key, {
    issuer: ISSUER,
    audience: AUDIENCE,
  });
  return payload as JWTClaims;
}
