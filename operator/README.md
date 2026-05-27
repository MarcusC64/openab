# oabctl — OAB Agent Provisioner for ECS

CLI tool that provisions and manages OpenAB agents on Amazon ECS Fargate.

## Quick Start

```bash
# Build
cd operator && cargo build --release

# Deploy an agent
oabctl apply -f examples/kiro-01.yaml

# List running agents
oabctl get oabservice

# Delete an agent
oabctl delete oabservice kiro-01 --cluster default --namespace prod
```

## Prerequisites

1. **AWS credentials** — IAM role/profile with permissions for ECS, SSM, S3
2. **S3 bucket** — `oab-control-plane` (manifests + rendered config)
3. **ECS cluster** — default cluster or specify with `--cluster`
4. **VPC** — subnets + security groups for Fargate tasks
5. **ECR image** — OAB container image pushed to ECR
6. **SSM parameters** — bot tokens stored at `/oab/{namespace}/{name}/discord-token`

## Manifest Schema

```yaml
apiVersion: oab.dev/v1
kind: OABService
metadata:
  name: kiro-01
  namespace: prod
spec:
  capacityProvider: FARGATE_SPOT   # FARGATE or FARGATE_SPOT
  cpu: 256                         # vCPU units
  memory: 512                      # MB
  taskDefinition:
    image: <ecr-image-uri>
  bootstrapFrom: s3://...          # agent HOME archive (memory, state)
  networking:
    subnets: [subnet-xxx]
    securityGroups: [sg-xxx]
  secrets:
    - name: DISCORD_TOKEN
      valueFrom: /oab/prod/kiro-01/discord-token
  config:
    channels:
      - type: discord
    backend:
      type: bedrock
      model_id: anthropic.claude-sonnet-4-20250514
      region: us-east-1
    steering:
      system_prompt: "..."
    features:
      stt: false
      cronjob: true
```

## Commands

| Command | Description |
|---------|-------------|
| `oabctl apply -f <file\|dir>` | Create or update agents from manifests |
| `oabctl get oabservice [name]` | List agents and their ECS status |
| `oabctl delete oabservice <name>` | Teardown agent (ECS + S3 cleanup) |

## How It Works

1. `oabctl apply` validates the manifest, renders `config.toml` from `spec.config`, uploads to S3 at an immutable path (`config/{ns}/{name}/{generation}/`), registers an ECS task definition, and creates/updates the ECS service.

2. ECS maintains the desired state — restarts failed tasks, handles rolling deployments. No separate controller needed.

3. On task startup, `entrypoint.sh` downloads the bootstrap archive and rendered config from S3, then starts OpenAB.

## Architecture

See [ADR: ECS Control Plane](../docs/adr/ecs-control-plane.md) for the full design.
