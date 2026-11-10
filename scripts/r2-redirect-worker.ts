// Cloudflare Worker in front of the wrightkit-release bucket. It serves bucket
// objects unchanged. A stable archive or checksum that retention pruned from R2
// redirects to the identical GitHub Release asset.

interface R2ObjectBody {
  body: ReadableStream;
  httpEtag: string;
  writeHttpMetadata(headers: Headers): void;
}

export interface Env {
  BUCKET: { get(key: string): Promise<R2ObjectBody | null> };
}

const PRUNED_STABLE =
  /^wright\/releases\/(\d+\.\d+\.\d+)\/(wright-\1-[A-Za-z0-9_-]+\.(?:tar\.gz|zip)(?:\.sha256)?)$/;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("method not allowed", { status: 405 });
    }
    const key = decodeURIComponent(new URL(request.url).pathname.slice(1));
    const object = await env.BUCKET.get(key);
    if (object) {
      const headers = new Headers();
      object.writeHttpMetadata(headers);
      headers.set("etag", object.httpEtag);
      return new Response(request.method === "HEAD" ? null : object.body, { headers });
    }
    const match = PRUNED_STABLE.exec(key);
    if (match) {
      const [, version, file] = match;
      return Response.redirect(
        `https://github.com/wrightkit/wright/releases/download/v${version}/${file}`,
        302,
      );
    }
    return new Response("not found", { status: 404 });
  },
};
