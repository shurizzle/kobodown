use std::{
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Parser;
use color_eyre::eyre::Report;
use dialoguer::MultiSelect;
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};
use kobodown::{Config, Kobo};
use zeroize::Zeroizing;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

#[derive(clap::Parser, Debug)]
enum Cli {
    Login(Login),
    Get(Get),
    List(List),
    Pick(Pick),
}

#[derive(clap::Parser, Debug)]
struct Login {
    #[arg(short, long)]
    pub username: Option<Box<str>>,
    #[arg(value_parser = password_parser, short, long)]
    pub password: Option<Zeroizing<Box<str>>>,
    #[arg(short, long)]
    pub captcha: Option<Box<str>>,
}

#[derive(clap::Parser, Debug)]
struct Get {
    #[arg(short = 'd', long)]
    pub output_dir: Option<PathBuf>,
    #[arg(short = 'o', long)]
    pub output_file: Option<PathBuf>,
    pub id: Box<str>,
}

#[derive(clap::Parser, Debug)]
struct List {
    #[arg(short, long, default_value_t = false)]
    pub all: bool,
}

#[derive(clap::Parser, Debug)]
struct Pick {
    #[arg(short = 'd', long)]
    pub output_dir: Option<PathBuf>,
    #[arg(short, long, default_value_t = false)]
    pub all: bool,
}

fn login(
    Login {
        username,
        password,
        captcha,
    }: Login,
) -> Result<(), Report> {
    fn read_line(prompt: &str) -> Result<Box<str>, Report> {
        let mut res = String::new();
        loop {
            {
                let mut w = std::io::stdout();
                w.write_all(prompt.as_bytes())?;
                w.flush()?;
            }

            std::io::stdin().read_line(&mut res)?;
            if matches!(res.chars().last(), Some('\n')) {
                res.truncate(res.len() - 1);
                if matches!(res.chars().last(), Some('\r')) {
                    res.truncate(res.len() - 1);
                }
            }
            if !res.is_empty() {
                return Ok(res.into_boxed_str());
            }
        }
    }

    fn read_password() -> Result<Zeroizing<Box<str>>, Report> {
        loop {
            let password = rpassword::prompt_password("Password: ")?;
            if !password.is_empty() {
                return Ok(Zeroizing::new(password.into_boxed_str()));
            }
        }
    }

    let username =
        if let Some(username) = username.and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            username
        } else {
            read_line("Username: ")?
        };
    let password =
        if let Some(password) = password.and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            password
        } else {
            read_password()?
        };
    let captcha = if let Some(captcha) =
        captcha.and_then(|s| if s.is_empty() { None } else { Some(s) })
    {
        captcha
    } else {
        println!(
            r#"
Open https://authorize.kobo.com/signin in a private/incognito window in your browser, wait till the page
loads (do not login!) then open the developer tools (use F12 in Firefox/Chrome), select the console tab,
and paste the following code there and then press Enter there in the browser.

var newCaptchaDiv = document.createElement( "div" );
newCaptchaDiv.id = "new-hcaptcha-container";
document.getElementById( "hcaptcha-container" ).insertAdjacentElement( "afterend", newCaptchaDiv );
hcaptcha.render( newCaptchaDiv.id, {{
	sitekey: "51a1773a-a9ae-4992-a768-e3b8d87355e8",
	callback: function( response ) {{ console.log( "Captcha response:" ); console.log( response ); }}
}} );

A captcha should show up below the Sign-in form. Once you solve the captcha its response will be written
below the pasted code in the browser's console. Copy the response (the line below "Captcha response:")
and paste it here.
            "#
        );
        read_line("Captcha: ")?
    };

    let mut config = Config::load();
    let mut kobo = Kobo::default();

    kobo.login(&mut config, &username, &password, &captcha)?;
    Ok(())
}

fn get(
    Get {
        output_dir,
        output_file,
        id,
    }: Get,
) -> Result<(), Report> {
    let mut settings = Config::load();
    let mut kobo = Kobo::default();

    let (output_dir, output_file) = if let Some(output_file) = output_file {
        if let Some(parent) = output_file.parent() {
            let name = output_file
                .file_name()
                .map(|s| Path::new(s).to_path_buf())
                .ok_or_else(|| color_eyre::eyre::eyre!("Invalid filename"))?;
            if parent.is_absolute() {
                (Some(parent.to_path_buf()), name)
            } else if let Some(mut output_dir) = output_dir {
                output_dir.push(parent);
                (Some(output_dir), name)
            } else {
                (Some(parent.to_path_buf()), name)
            }
        } else {
            if output_file.file_name().is_none() {
                color_eyre::eyre::bail!("Invalid filename");
            }
            (output_dir, output_file)
        }
    } else {
        let book = kobo.book_info(&mut settings, &id)?;
        (
            output_dir,
            PathBuf::from(mkname(book.author.as_deref(), &book.title)),
        )
    };

    let desc = kobo.access_book(&mut settings, &id)?;
    let pb = default_bar(None);
    download_zip(
        &mut kobo,
        &mut settings,
        desc,
        output_dir.as_deref(),
        output_file,
        &pb,
        DownloadProgress(None),
    )?;
    Ok(())
}

fn pick(Pick { output_dir, all }: Pick) -> Result<(), Report> {
    let mut config = Config::load();
    let mut kobo = Kobo::default();
    let mut books = kobo.book_list(&mut config, all)?;

    let selections = MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .items(&books)
        .interact_opt()?;
    let Some(mut selections) = selections else {
        return Ok(());
    };
    match selections.len() {
        0 => return Ok(()),
        1 => {
            let book = books.remove(selections[0]);
            return get(Get {
                output_dir,
                output_file: Some(mkname(book.authors.as_deref(), &book.title).into()),
                id: book.revision_id,
            });
        }
        _ => (),
    }
    selections.sort_unstable();

    let pb = MultiProgress::new();
    let file_pb = pb.add(ProgressBar::no_length().with_finish(indicatif::ProgressFinish::AndClear));
    let global_pb = pb.add(
        ProgressBar::new(selections.len() as u64 * 2)
            .with_finish(indicatif::ProgressFinish::AndClear)
            .with_style(global_style()),
    );
    pb.clear()?;
    global_pb.enable_steady_tick(DEFAULT_TICK);

    for book in selections.into_iter().flat_map(|i| books.get(i)) {
        let desc = kobo.access_book(&mut config, &book.revision_id)?;
        let file = mkname(book.authors.as_deref(), &book.title);
        download_zip(
            &mut kobo,
            &mut config,
            desc,
            output_dir.as_deref(),
            PathBuf::from(file),
            &file_pb,
            DownloadProgress(Some(&global_pb)),
        )?;
    }
    Ok(())
}

fn list(List { all }: List) -> Result<(), Report> {
    let mut config = Config::load();
    let mut kobo = Kobo::default();

    for book in kobo.book_list(&mut config, all)? {
        println!("{} - {book}", book.revision_id);
    }
    Ok(())
}

fn password_parser(raw: &str) -> Result<Zeroizing<Box<str>>, Infallible> {
    Ok(Zeroizing::new(raw.to_string().into_boxed_str()))
}

fn main() -> Result<(), Report> {
    #[cfg(debug_assertions)]
    {
        use tracing_error::ErrorLayer;
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::{fmt, EnvFilter};

        let fmt_layer = fmt::layer().with_target(false);
        let filter_layer = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("info"))
            .unwrap();

        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .with(ErrorLayer::default())
            .init();
    }
    color_eyre::install()?;

    match Cli::parse() {
        Cli::Login(args) => login(args),
        Cli::Get(args) => get(args),
        Cli::List(args) => list(args),
        Cli::Pick(args) => pick(args),
    }?;
    Ok(())
}

const DEFAULT_TICK: Duration = Duration::from_millis(100);

fn myperc(s: &ProgressState, w: &mut dyn core::fmt::Write) {
    write!(w, "{:.*}%", 1, s.fraction() * 100f32).unwrap();
}

fn mypersec(s: &ProgressState, w: &mut dyn core::fmt::Write) {
    write!(w, "{:.*}/s", 1, s.per_sec()).unwrap();
}

fn bar_style(template: &str) -> ProgressStyle {
    ProgressStyle::with_template(template)
        .unwrap()
        .with_key("myperc", myperc)
        .with_key("mypersec", mypersec)
        .progress_chars("━╸━")
}

fn download_style() -> ProgressStyle {
    const TEMPLATE: &str = "{wide_msg:.blue.bold}\n{spinner:.green} {wide_bar:.magenta.bright/black.bright} {myperc:.magenta} • {bytes_per_sec:.red} • {eta:.cyan}";
    bar_style(TEMPLATE)
}

fn decrypt_style() -> ProgressStyle {
    const TEMPLATE: &str = "{wide_msg:.blue.bold}\n{spinner:.green} {wide_bar:.magenta.bright/black.bright} {myperc:.magenta} • {mypersec:.red} • {eta:.cyan}";
    bar_style(TEMPLATE)
}

fn global_style() -> ProgressStyle {
    const TEMPLATE: &str =
        "  {wide_bar:.magenta.bright/black.bright} {myperc:.magenta} • {eta:.cyan}";
    bar_style(TEMPLATE)
}

fn default_bar(length: Option<u64>) -> ProgressBar {
    let pb = length
        .map(ProgressBar::new)
        .unwrap_or_else(ProgressBar::no_length)
        .with_finish(indicatif::ProgressFinish::AndClear);
    pb.enable_steady_tick(DEFAULT_TICK);
    pb
}

#[derive(Debug)]
pub struct TempFile {
    key: usize,
    inner: Option<File>,
}

impl TempFile {
    fn inner(&mut self) -> &mut File {
        self.inner.as_mut().unwrap()
    }

    pub fn set_len(&self, size: u64) -> std::io::Result<()> {
        self.inner.as_ref().unwrap().set_len(size)
    }
}

impl Write for TempFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner().flush()
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.inner().write_vectored(bufs)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.inner().write_all(buf)
    }

    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        self.inner().write_fmt(fmt)
    }
}

impl Seek for TempFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner().seek(pos)
    }

    fn rewind(&mut self) -> std::io::Result<()> {
        self.inner().rewind()
    }

    fn stream_position(&mut self) -> std::io::Result<u64> {
        self.inner().stream_position()
    }

    fn seek_relative(&mut self, offset: i64) -> std::io::Result<()> {
        self.inner().seek_relative(offset)
    }
}

impl Read for TempFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner().read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize> {
        self.inner().read_vectored(bufs)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        self.inner().read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize> {
        self.inner().read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.inner().read_exact(buf)
    }
}

mod tmp {
    use std::{
        collections::HashMap,
        ffi::OsStr,
        fs::File,
        mem::ManuallyDrop,
        path::{Path, PathBuf},
        sync::LazyLock,
    };

    use color_eyre::eyre::Report;
    use parking_lot::Mutex;

    static CLEANUP_FILES: LazyLock<Mutex<Cleanup>> = LazyLock::new(|| {
        ctrlc::set_handler(destroy).unwrap();

        Mutex::new(Cleanup {
            index: 0,
            files: HashMap::new(),
        })
    });

    fn destroy() {
        unsafe { &mut *CLEANUP_FILES.data_ptr() }.finalize();
        std::process::exit(130);
    }

    struct Cleanup {
        index: usize,
        files: HashMap<usize, PathBuf>,
    }

    impl Cleanup {
        pub fn insert(&mut self, path: PathBuf) -> usize {
            loop {
                let index = self.index;
                self.index += self.index.wrapping_add(1);
                match self.files.entry(index) {
                    std::collections::hash_map::Entry::Occupied(_) => (),
                    std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(path);
                        return index;
                    }
                }
            }
        }

        pub fn remove(&mut self, index: usize) {
            _ = std::fs::remove_file(self.ignore(index));
        }

        pub fn ignore(&mut self, index: usize) -> PathBuf {
            self.files.remove(&index).unwrap()
        }

        pub fn finalize(&mut self) {
            for (_, path) in core::mem::take(&mut self.files) {
                _ = std::fs::remove_file(path);
            }
        }
    }

    impl super::TempFile {
        pub fn with_prefix_in<S: AsRef<OsStr>, P: AsRef<Path>>(
            suffix: S,
            dir: P,
        ) -> Result<super::TempFile, Report> {
            let (f, path) = tempfile::NamedTempFile::with_prefix_in(suffix, dir)?.keep()?;
            Ok(Self::from_parts(f, path))
        }

        pub fn from_parts(f: File, path: PathBuf) -> Self {
            let key = CLEANUP_FILES.lock().insert(path);

            super::TempFile {
                key,
                inner: Some(f),
            }
        }

        pub fn keep(self) -> File {
            let me = ManuallyDrop::new(self);
            let key = me.key;
            let inner = unsafe { core::ptr::read(&me.inner).unwrap() };
            CLEANUP_FILES.lock().ignore(key);
            inner
        }
    }

    impl Drop for super::TempFile {
        fn drop(&mut self) {
            drop(self.inner.take());
            CLEANUP_FILES.lock().remove(self.key);
        }
    }
}

pub struct DownloadProgress<'a>(Option<&'a ProgressBar>);
pub struct DecryptProgress<'a>(Option<&'a ProgressBar>);

impl<'a> DownloadProgress<'a> {
    pub fn step(mut self) -> DecryptProgress<'a> {
        DecryptProgress(self.0.take().inspect(|pb| pb.inc(1)))
    }
}

impl DecryptProgress<'_> {
    pub fn step(mut self) {
        if let Some(pb) = self.0.take() {
            pb.inc(1);
        }
    }
}

impl Drop for DownloadProgress<'_> {
    fn drop(&mut self) {
        if let Some(pb) = self.0.take() {
            pb.inc(2);
        }
    }
}

impl Drop for DecryptProgress<'_> {
    fn drop(&mut self) {
        if let Some(pb) = self.0.take() {
            pb.inc(1);
        }
    }
}

fn download_zip<T, S, P1, P2>(
    kobo: &mut Kobo<T>,
    session: &mut S,
    kobodown::AccessBook {
        size,
        content_keys,
        url,
    }: kobodown::AccessBook,
    dir: Option<P1>,
    name: P2,
    pb: &ProgressBar,
    progress: DownloadProgress<'_>,
) -> Result<(), Report>
where
    T: kobodown::Transport,
    S: kobodown::Session,
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    pb.disable_steady_tick();
    pb.reset();
    pb.set_message(format!("Downloading {}...", name.as_ref().display()));
    pb.set_style(download_style());

    if let Some(content_keys) = content_keys {
        pb.update(|ps| {
            ps.set_len(size * 2);
            ps.set_pos(0);
        });
        pb.enable_steady_tick(DEFAULT_TICK);

        let mut tmp = if let Some(dir) = dir.as_ref() {
            std::fs::create_dir_all(dir)?;
            TempFile::with_prefix_in(name.as_ref(), dir)
        } else {
            TempFile::with_prefix_in(name.as_ref(), ".")
        }?;
        tmp.set_len(size)?;

        kobo.download(session, &url, pb.wrap_write(&mut tmp))?;
        let _progress = progress.step();
        tmp.seek(std::io::SeekFrom::Start(0))?;
        let path = if let Some(dir) = dir.as_ref() {
            dir.as_ref().join(name.as_ref())
        } else {
            name.as_ref().into()
        };
        let f = File::create(&path)?;
        f.set_len(size)?;
        let mut f = TempFile::from_parts(f, path);

        decrypt_zip(&content_keys, &mut tmp, &mut f, name, pb)?;
        f.keep();
    } else {
        pb.update(|ps| {
            ps.set_len(size);
            ps.set_pos(0);
        });
        pb.enable_steady_tick(DEFAULT_TICK);

        let path = if let Some(dir) = dir.as_ref() {
            dir.as_ref().join(name)
        } else {
            name.as_ref().into()
        };
        let f = File::create(&path)?;
        let mut f = TempFile::from_parts(f, path);
        kobo.download(session, &url, pb.wrap_write(&mut f))?;
        f.keep();
    }
    Ok(())
}

fn decrypt_zip<R: Read + Seek, W: Write + Seek, P: AsRef<Path>>(
    keys: &HashMap<Box<str>, aes::cipher::Key<aes::Aes128Dec>>,
    input: &mut R,
    output: &mut W,
    name: P,
    pb: &ProgressBar,
) -> Result<(), Report> {
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::DEFLATE);
    let mut ziparchive = ZipWriter::new(output);
    let mut inzip = ZipArchive::new(input)?;
    pb.disable_steady_tick();
    pb.reset();
    pb.set_style(decrypt_style());
    pb.set_message(format!("Decrypting {}...", name.as_ref().display()));
    pb.update(|ps| {
        ps.set_len(inzip.len() as u64 * 2);
        ps.set_pos(inzip.len() as u64);
    });
    pb.enable_steady_tick(DEFAULT_TICK);

    for i in 0..inzip.len() {
        let mut infile = inzip.by_index(i)?;
        ziparchive.start_file(infile.name(), options)?;
        if let Some(key) = keys.get(infile.name()) {
            // PERF: ???
            let mut data = Vec::with_capacity(infile.size() as usize);
            infile.read_to_end(&mut data)?;
            ziparchive.write_all(aes::cipher::BlockDecryptMut::decrypt_padded_mut::<
                aes::cipher::block_padding::Pkcs7,
            >(
                <ecb::Decryptor<aes::Aes128> as aes::cipher::KeyInit>::new(key),
                &mut data,
            )?)?;
        } else {
            std::io::copy(&mut infile, &mut ziparchive)?;
        }
        pb.inc(1);
    }
    ziparchive.finish()?;
    Ok(())
}

fn mkname(author: Option<&str>, title: &str) -> String {
    let mut name;
    if let Some(author) = author.and_then(|a| if a.is_empty() { None } else { Some(a) }) {
        name = sanitize_filename::sanitize(author);
        let title = sanitize_filename::sanitize(title);
        name.reserve_exact(title.len() + 8);
        name.push_str(" - ");
        name.push_str(&title);
    } else {
        name = sanitize_filename::sanitize(title);
        name.reserve_exact(5);
    }
    name.push_str(".epub");
    name
}
