# Corvus on Kubernetes

These manifests deploy one Corvus daemon in the `corvus` namespace with a
1 GiB ReadWriteOnce volume. The image is `corvus:1.7.0` and listens on
7330 (WebSocket) and 7331 (MCP HTTP).

## Prerequisites

- A Kubernetes cluster with a default StorageClass.
- The image available to the cluster. For a local cluster, build and load it:

  ```bash
  docker build -t corvus:1.7.0 .
  # kind load docker-image corvus:1.7.0
  # minikube image load corvus:1.7.0
  ```

  For a remote cluster, push the image to a registry and change the image
  reference in `deployment.yaml`.

## Apply

```bash
kubectl apply -k deploy/k8s
kubectl -n corvus rollout status deployment/corvus
kubectl -n corvus get pods,svc,pvc
```

## Access

The Service is ClusterIP. Forward both ports to the local machine:

```bash
kubectl -n corvus port-forward svc/corvus 7330:7330 7331:7331
curl -fsS http://127.0.0.1:7331/health
```

The daemon is ready when `/health` returns status ok and the expected
version. MCP clients use `http://127.0.0.1:7331/mcp`; the browser extension
uses `ws://127.0.0.1:7330`.

The NetworkPolicy default-denies ingress, then allows TCP 7330 and 7331 only
from pods in the corvus namespace and from pods that match both the
`corvus.io/allow-client: "true"` namespace label and the
`corvus.io/client: "true"` pod label. Egress is DNS only to
kube-dns. A browser extension cannot reach a ClusterIP directly; desktop
clients should use the port-forward above, or an authenticated ingress or
proxy that you operate. Same-namespace clients need no extra label.

## Data and updates

The PVC holds the recording database under `/data/data/corvus.db`, logs under
`/data/logs`, config under `/data/config`, and the unpacked browser
extension under `/data/extension`. The deployment uses one replica because
the volume is ReadWriteOnce.

An init container seeds /data/extension from /opt/corvus/extension on every
pod start, so an existing volume receives the extension from the current
image. Reload the extension in the browser after upgrading the deployment.

## Security

Corvus has no authentication. Keep the Service at ClusterIP and use
`kubectl port-forward` from a trusted workstation. Do not publish these
ports through a public LoadBalancer or Ingress. The container runs as UID/GID
10001 with a read-only root filesystem, no Linux capabilities, and a
RuntimeDefault seccomp profile.
