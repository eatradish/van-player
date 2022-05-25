use anyhow::{anyhow, Ok, Result};
use youtube_dl::YoutubeDl;

#[derive(Debug)]
struct YtdlMediaMeta {
    title: Option<String>,
    uploader: Option<String>,
}

fn get_youtubedl_info(
    queue: Vec<&str>,
    thread: Option<usize>,
) -> Result<Vec<Result<YtdlMediaMeta>>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(thread.unwrap_or(2))
        .build()?;

    runtime.block_on(async move {
        let mut tasks = Vec::new();
        for url in queue {
            tasks.push(get_youtubedl_info_inner(url));
        }
        let results = futures::future::join_all(tasks).await;

        Ok(results)
    })
}

async fn get_youtubedl_info_inner(url: &str) -> Result<YtdlMediaMeta> {
    let output = YoutubeDl::new(url).socket_timeout("15").run_async().await?;
    let (title, uploader) = if let Some(output) = output.clone().into_playlist() {
        let title = output.title;
        let uploader = if let Some(uploader) = output.uploader {
            Some(uploader)
        } else {
            output
                .entries
                .and_then(|x| x.first().and_then(|x| x.uploader.clone()))
        };

        (title, uploader)
    } else {
        let output = output
            .into_single_video()
            .ok_or_else(|| anyhow!("Can not get {} info!", url))?;
        let title = Some(output.title);
        let uploader = output.uploader;

        (title, uploader)
    };

    Ok(YtdlMediaMeta { title, uploader })
}

#[tokio::test]
async fn test() {
    dbg!(
        get_youtubedl_info_inner("https://www.bilibili.com/video/BV1NY4y1t7hx?p=7")
            .await
            .unwrap()
    );
}

#[test]
fn test_get_list_info() {
    let queue = vec![
        "https://www.bilibili.com/video/BV1WL4y1F7Uj",
        "https://www.bilibili.com/video/BV1AF411571Z",
        "https://www.bilibili.com/video/BV1NY4y1t7hx?p=7",
    ];
    dbg!(get_youtubedl_info(queue, None).unwrap());
}
