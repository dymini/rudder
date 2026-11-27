# dymini-panel-cache image, built from the static release binary dist/dymini-panel-cache-<arch>.
#
# docker buildx build -f deploy/docker/cache.Dockerfile --platform linux/amd64,linux/arm64 .
FROM scratch

ARG TARGETARCH
COPY dist/dymini-panel-cache-${TARGETARCH} /dymini-panel-cache

USER 65532:65532

EXPOSE 8080 9090
ENTRYPOINT ["/dymini-panel-cache"]
CMD ["--rules", "/var/lib/dymini-panel/cache/rules.json", "--listen", "0.0.0.0:8080", "--admin-listen", "0.0.0.0:9090"]
