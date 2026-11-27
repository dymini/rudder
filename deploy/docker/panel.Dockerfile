# Dymini Panel image, built from release binaries.
#
#   dist/dymini-panel-server-<arch>                     static control-plane binary (web UI embedded)
#   dist/agents/dymini-panel-agent-<triple>[.sha256]    agent binaries served at /downloads
#
# docker buildx build -f deploy/docker/panel.Dockerfile --platform linux/amd64,linux/arm64 .
FROM alpine:3.22

RUN apk add --no-cache \
        ca-certificates tzdata \
        git openssh-client \
        docker-cli docker-cli-buildx docker-cli-compose \
        postgresql16-client

ARG TARGETARCH
COPY dist/dymini-panel-server-${TARGETARCH} /usr/local/bin/dymini-panel-server
COPY dist/agents/ /usr/share/dymini-panel/dist/

ENV DYMINI_PANEL_BIND=0.0.0.0:3000 \
    DYMINI_PANEL_DATA_DIR=/var/lib/dymini-panel \
    DYMINI_PANEL_DIST_DIR=/usr/share/dymini-panel/dist \
    DYMINI_PANEL_HOST_PROC=/host/proc

EXPOSE 3000 7443
ENTRYPOINT ["/usr/local/bin/dymini-panel-server"]
