/*
 * cli/src/main.rs
 */

mod tui;

use std::{
    io,
    time::{
        Duration,
        Instant,
    },
};
use clap::{
    ArgGroup,
    Parser,
};
use crossterm::{
    event,
    event::{
        Event,
        KeyCode,
        KeyEvent,
        KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{
        disable_raw_mode,
        enable_raw_mode,
        EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

use core::{
    Config,
    GameMode,
    Level,
    RawResults,
    process_results,
    language_from_str,
    SCHEMES_DIR,
    generate_content,
    list_languages,
    list_schemes,
    validate_config,
    Test
};

use tui::{
    TestView,
    ResultView,
    StartView,
    load_scheme_file
};

use core::results::{Key};

fn convert_key(key: &KeyEvent) -> Key {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Key::CtrlC;
    }

    match key.code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        other => Key::Other(format!("{:?}", other)),
    }
}


const STYLE_ERROR: &str = "\x1b[1;31merror:\x1b[0m";        // 1;31 = bold red, 0m = reset
const _STYLE_WARNING: &str = "\x1b[1;33mwarning:\x1b[0m";    // bold yellow
const _STYLE_INFO: &str = "\x1b[1;32minfo:\x1b[0m";          // bold green


#[derive(Debug, Parser)]
#[command(
    name = "typecrab",
    about = "A minimalistic, customizable typing test.",
    version
)]
#[command(group(
    ArgGroup::new("mode")
        .args(&["words", "quote", "zen"])
        .multiple(false)
))]
#[command(group(
    ArgGroup::new("language_source")
        .args(&["language", "language_file"])
        .multiple(false)
))]
#[command(group(
    ArgGroup::new("scheme_source")
        .args(&["scheme", "scheme_file"])
        .multiple(false)
))]
#[command(group(
    ArgGroup::new("listing")
        .args(&["list_languages", "list_schemes"])
        .multiple(false)
))]
struct Opt {
    /// List available languages
    #[arg(long = "list-languages")]
    list_languages: bool,

    /// List available color schemes
    #[arg(long = "list-schemes")]
    list_schemes: bool,

    /// Enable words mode [default]
    #[arg(short, long)]
    words: bool,

    /// Enable quote mode
    #[arg(short, long)]
    quote: bool,

    /// Enable zen mode
    #[arg(short, long)]
    zen: bool,

    /// Include punctuation in test text
    #[arg(short, long)]
    punctuation: bool,

    /// Include numbers in test text
    #[arg(short, long)]
    numbers: bool,

    /// Disable backtracking of completed words
    #[arg(long = "strict", action = clap::ArgAction::SetTrue)]
    strict: bool,

    /// Enable sudden death on first mistake
    #[arg(long)]
    death: bool,

    /// Specify test language
    #[arg(short, long, value_name = "lang", default_value = "en")]
    language: String,

    /// Specify custom test file
    #[arg(long = "language-file", value_name = "path")]
    language_file: Option<String>,

    /// Specify color scheme
    #[arg(short, long, value_name = "lang", default_value = "monokai")]
    scheme: String,

    /// Specify custom color scheme file
    #[arg(long = "scheme-file", value_name = "path")]
    scheme_file: Option<String>,

    /// Specify word count
    #[arg(short, long, value_name = "n", default_value_t = 25)]
    count: usize,

    /// Specify time limit
    #[arg(short, long, value_name = "sec")]
    time: Option<u32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {

    // arguments parsing
    let opt = Opt::parse();

    // listing = end
    if opt.list_languages || opt.list_schemes {
        let response = if opt.list_languages {
            list_languages()
        } else {
            list_schemes()
        };

        if let Some((Level::Error, msg)) = &response.message {
            eprintln!("{STYLE_ERROR} {msg}");
            std::process::exit(1);
        }

        for item in &response.payload {
            println!("{item}");
        }

        return Ok(());
    }

    // color scheme configuration
    if let Some(path) = &opt.scheme_file {
        if let Err(msg) = load_scheme_file(path) {
            eprintln!("{STYLE_ERROR} {msg}");
            std::process::exit(1);
        }
    } else {
        let path = format!("{}/{}.css", SCHEMES_DIR, opt.scheme);
        if let Err(msg) = load_scheme_file(&path) {
            eprintln!("{STYLE_ERROR} {msg}");
            std::process::exit(1);
        }
    }

    // initial config
    let mode = if opt.quote {
        GameMode::Quote
    } else if opt.zen {
        GameMode::Zen
    } else {
        GameMode::Words
    };

    let initial_config = Config {
        mode,
        language: language_from_str(&opt.language, mode),
        file: opt.language_file,
        word_count: opt.count,
        time_limit: opt.time,
        punctuation: opt.punctuation,
        numbers: opt.numbers,
        backtrack: !opt.strict,
        death: opt.death,
    };

    // api config validation
    let config_response = validate_config(initial_config);

    if let Some((Level::Error, msg)) = &config_response.message {
        eprintln!("{STYLE_ERROR} {msg}");
        std::process::exit(1);
    }

    let config = config_response.payload;

    // api words generation
    let generation_response = generate_content(&config);

    if let Some((Level::Error, msg)) = &generation_response.message {
        eprintln!("{STYLE_ERROR} {msg}");
        std::process::exit(1);
    }

    let words = &generation_response.payload;

    // new test
    let mut test = Test::new(words.clone(), &config);

    // entering tui
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // drawing start screen
    loop {
        terminal.draw(|f| {
            let size = f.area();
            f.render_widget(StartView, size);
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(_) => break,
                Event::Resize(_, _) => continue,
                _ => {}
            }
        }
    }
    
    // choosing what warning to show
    let response_message = config_response.message.clone().or(generation_response.message.clone());
    let mut warning_message = response_message.clone();

    let test_start = Instant::now();

    // main test cycle
    loop {
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {   // processing entered key
                test.handle_key(convert_key(&key));

                // hot keys
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break
                        }
                        _ => {}
                    }
                }
            }
        }

        // test complete = test end
        if test.complete {
            break;
        }

        // time end = test end
        let elapsed = test_start.elapsed().as_secs();

        if let Some(limit) = config.time_limit {
            let time_left = limit as i64 - elapsed as i64;
            if time_left <= 0 {
                break;
            }
        }

        // warning display - maximum priority
        if let Some(_) = warning_message {
            if test_start.elapsed().as_secs() >= 3 {
                warning_message = None;
            }
        }

        // status display
        let status_string: Option<String> = if warning_message.is_some() {  // priority - warning message
            None
        } else if let Some(limit) = config.time_limit {     // next - time
            let time_left = limit as i64 - elapsed as i64;
            Some(time_left.to_string())
        } else {                                // next - words
            if config.mode == GameMode::Zen {
                Some(test.current_word.to_string())
            }
            else {
                Some(format!("{}/{}", test.current_word, test.words.len()))
            }
        };

        // rendering current state
        terminal.draw(|f| {
            let size = f.area();
            let view = TestView {
                test: &test,
                status: status_string.clone(),
                warning: warning_message.clone(),
            };
            f.render_widget(view, size);
        })?;

    }

    // zen mode = exit, because sensitive psyche of zen mod user will not tolerate his horrifying erroneous results
    if config.mode != GameMode::Zen {
        let raw_results = RawResults::from(&test);

        // api final results generation from raw test results
        let final_results = process_results(raw_results).payload;

        // render results
        loop {
            terminal.draw(|f| {
                let size = f.area();
                let view = ResultView { results: &final_results };
                f.render_widget(view, size);
            })?;

            if event::poll(Duration::from_millis(50))? {
                match crossterm::event::read()? {
                    Event::Key(_) => break,
                    Event::Resize(_, _) => continue,
                    _ => {}
                }
            }
        }
    }

    // exiting tui
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}
