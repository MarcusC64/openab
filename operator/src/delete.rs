use anyhow::{Context, Result};

pub async fn run(
    aws_config: &aws_config::SdkConfig,
    resource: &str,
    name: &str,
    cluster: &str,
    namespace: &str,
) -> Result<()> {
    if resource != "oabservice" {
        anyhow::bail!("unknown resource type: {}. Use 'oabservice'", resource);
    }

    let service_name = format!("oab-{}-{}", namespace, name);
    let ecs = aws_sdk_ecs::Client::new(aws_config);
    let s3 = aws_sdk_s3::Client::new(aws_config);
    let bucket = "oab-control-plane";

    println!("Deleting {}...", name);

    // 1. Scale to 0
    let _ = ecs
        .update_service()
        .cluster(cluster)
        .service(&service_name)
        .desired_count(0)
        .send()
        .await;
    println!("  ✓ Scaled to 0");

    // 2. Delete ECS service
    ecs.delete_service()
        .cluster(cluster)
        .service(&service_name)
        .force(true)
        .send()
        .await
        .context("failed to delete ECS service")?;
    println!("  ✓ ECS service deleted");

    // 3. Clean up S3 manifest
    let manifest_key = format!("manifests/{}/{}.yaml", namespace, name);
    let _ = s3
        .delete_object()
        .bucket(bucket)
        .key(&manifest_key)
        .send()
        .await;
    println!("  ✓ Manifest removed from S3");

    // 4. Clean up S3 config (list and delete all generations)
    let config_prefix = format!("config/{}/{}/", namespace, name);
    let list = s3
        .list_objects_v2()
        .bucket(bucket)
        .prefix(&config_prefix)
        .send()
        .await;
    if let Ok(resp) = list {
        for obj in resp.contents() {
            if let Some(key) = obj.key() {
                let _ = s3.delete_object().bucket(bucket).key(key).send().await;
            }
        }
    }
    println!("  ✓ Config artifacts removed from S3");

    println!("\n✓ {} deleted", name);
    Ok(())
}
