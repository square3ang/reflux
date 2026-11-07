//! The Media Foundation player alone, for `scripts/mf-wine-test.sh` (which fills in MF_RS).
#[path = "MF_RS"]
#[allow(dead_code)] // rate and volume controls the tests don't touch
mod mf;

fn main() {}

#[cfg(test)]
mod wine {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::mf::VideoPlayer;

    /// Frames seen and the last one's width, height and RGBA.
    type Frames = Arc<Mutex<(u64, u32, u32, Vec<u8>)>>;

    fn open(env: &str) -> (VideoPlayer, Frames) {
        let frames: Frames = Arc::default();
        let f = frames.clone();
        let p = VideoPlayer::new(&std::env::var(env).unwrap(), move |w, h, rgba| {
            let mut f = f.lock().unwrap();
            *f = (f.0 + 1, w, h, rgba);
        })
        .expect("Media Foundation starts");
        (p, frames)
    }

    fn until(p: &mut VideoPlayer, what: &str, f: impl Fn(&VideoPlayer) -> bool) {
        let t = Instant::now();
        while !f(p) {
            p.poll();
            assert!(t.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
        eprintln!("  {what}: {:.2}s, position {:?}", t.elapsed().as_secs_f64(), p.position());
    }

    #[test]
    fn mp4_plays_pauses_seeks_ends_and_loops() {
        let (mut p, frames) = open("MF_CLIP_AV");
        let n = || frames.lock().unwrap().0;
        p.play();
        until(&mut p, "frames and sound", |p| n() > 3 && p.audio_samples() > 4800);
        {
            let f = frames.lock().unwrap();
            // videotestsrc's second bar is yellow: catches a BGR/RGB mix-up.
            let px = &f.3[(10 * 320 + 68) * 4..][..4];
            eprintln!("  frame {}x{} ({} bytes), yellow bar {px:?}, duration {:?}", f.1, f.2, f.3.len(), p.duration());
            assert_eq!((f.1, f.2, f.3.len()), (320, 240, 320 * 240 * 4));
            assert!(px[0] > 150 && px[1] > 150 && px[2] < 80 && px[3] == 255, "{px:?}");
        }
        assert!(p.duration().is_some_and(|d| (d - 2.0).abs() < 0.15));
        p.pause();
        std::thread::sleep(Duration::from_millis(100));
        let (pos, count) = (p.position(), n());
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!((p.position(), n()), (pos, count), "paused clip moved");
        // Paused, a seek shows its frame.
        p.seek(1.5);
        until(&mut p, "the seek frame", |p| n() > count && p.position().is_some_and(|x| (x - 1.5).abs() < 0.05));
        p.play();
        until(&mut p, "the end", |p| p.ended);
        assert!(!p.playing);
        let count = n();
        p.play();
        until(&mut p, "the replay", |_| n() > count + 3);
        assert!(p.position().is_some_and(|x| x < 1.0));
    }

    /// Several at once, like gifv tiles (wine deadlocked on concurrent opens before they were serialized).
    #[test]
    fn four_at_once() {
        let mut ps: Vec<_> = (0..4).map(|_| open("MF_CLIP_AV")).collect();
        for (p, _) in &mut ps {
            p.set_volume(0.0, true);
            p.play();
        }
        for (p, f) in &mut ps {
            let f = f.clone();
            until(p, "frames", move |_| f.lock().unwrap().0 > 3);
        }
    }

    #[test]
    fn mp3_audio_only() {
        let (mut p, frames) = open("MF_CLIP_MP3");
        p.set_volume(0.5, false);
        p.play();
        until(&mut p, "sound", |p| p.audio_samples() > 4410);
        until(&mut p, "the end", |p| p.ended);
        assert_eq!(frames.lock().unwrap().0, 0);
        eprintln!("  samples {}, duration {:?}", p.audio_samples(), p.duration());
    }
}
