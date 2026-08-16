FROM node:22-bookworm-slim AS builder
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-workspace.yaml turbo.json pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile
COPY apps/web apps/web
RUN pnpm --filter @nopager/web build

FROM node:22-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production \
    HOSTNAME=0.0.0.0 \
    PORT=3000
RUN corepack enable
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --prod --frozen-lockfile
COPY --from=builder /app/apps/web/.next ./apps/web/.next
COPY --from=builder /app/apps/web/public ./apps/web/public
EXPOSE 3000
CMD ["pnpm", "--filter", "@nopager/web", "start"]
