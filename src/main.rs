use libc::*;
use ncurses::*;
use regex::Regex;
use std::error::Error;
use std::ffi::CString;
use std::fs::File;
use std::io::stdin;
use std::process::Command;

#[derive(Debug)]
struct Line {
    text: String,
    // TODO: Line::caps should vector of ranges of usize
    caps: Vec<(usize, usize)>,
}

impl Line {
    fn from_string(text: &str) -> Self {
        Self {
            text: String::from(text),
            caps: Vec::new(),
        }
    }
}

const REGULAR_PAIR: i16 = 1;
const CURSOR_PAIR: i16 = 2;
const MATCH_PAIR: i16 = 3;
const MATCH_CURSOR_PAIR: i16 = 4;

fn render_status(text: &str) {
    let h = {
        let mut x: i32 = 0;
        let mut y: i32 = 0;
        getmaxyx(stdscr(), &mut y, &mut x);
        y
    };

    if h <= 1 {
        mv(0, 0);
        addstr("MAKE THE WINDOW BIGGER YOU FOOL!");
    } else {
        mv(h - 1, 0);
        addstr(text);
    }
}

fn render_list(lines: &[Line], cursor_y: usize, cursor_x: usize) {
    let (w, h) = {
        let mut x: i32 = 0;
        let mut y: i32 = 0;
        getmaxyx(stdscr(), &mut y, &mut x);
        (x as usize, y as usize - 1)
    };

    if h > 0 {
        // TODO(#16): word wrapping for long lines
        for (i, line) in lines.iter().skip(cursor_y / h * h).enumerate().take_while(|(i, _)| *i < h) {
            let line_to_render = {
                let mut line_to_render = line
                    .text
                    .trim_end()
                    .get(cursor_x..)
                    .unwrap_or("")
                    .to_string();
                let n = line_to_render.len();
                if n < w {
                    for _ in 0..(w - n) {
                        line_to_render.push(' ');
                    }
                }
                line_to_render
            };

            mv(i as i32, 0);
            let (pair, cap_pair) = if i == (cursor_y % h) {
                (CURSOR_PAIR, MATCH_CURSOR_PAIR)
            } else {
                (REGULAR_PAIR, MATCH_PAIR)
            };
            attron(COLOR_PAIR(pair));
            addstr(&line_to_render);
            attroff(COLOR_PAIR(pair));

            for (start0, end0) in &line.caps {
                let start = usize::max(cursor_x, *start0);
                let end = usize::min(cursor_x + w, *end0);
                if start != end {
                    mv(i as i32, (start - cursor_x) as i32);
                    attron(COLOR_PAIR(cap_pair));
                    addstr(line.text.get(start..end).unwrap_or(""));
                    attroff(COLOR_PAIR(cap_pair));
                }
            }
        }
    }
}

#[derive(Debug)]
struct Profile {
    regexs: Vec<String>,
    cmds: Vec<String>,
    current_regex: usize,
    current_cmd: usize,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            regexs: vec![r"^(.*?):(\d+):".to_string()],
            cmds: vec!["vim +\\2 \\1".to_string()],
            current_regex: 0,
            current_cmd: 0,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let profile = Profile::default();

    let re = Regex::new(profile.regexs[profile.current_regex].as_str())?;

    let mut lines: Vec<Line> = Vec::new();
    let mut cursor_x: usize = 0;
    let mut cursor_y: usize = 0;
    let mut line_text: String = String::new();
    while stdin().read_line(&mut line_text)? > 0 {
        let caps = re.captures_iter(line_text.as_str()).next();
        let mut line = Line::from_string(line_text.as_str());

        for cap in caps {
            for mat_opt in cap.iter().skip(1) {
                if let Some(mat) = mat_opt {
                    line.caps.push((mat.start(), mat.end()))
                }
            }
        }

        lines.push(line);
        line_text.clear();
    }

    // NOTE: stolen from https://stackoverflow.com/a/44884859
    // TODO(#3): the terminal redirection is too hacky
    let tty_path = CString::new("/dev/tty")?;
    let fopen_mode = CString::new("r+")?;
    let file = unsafe { fopen(tty_path.as_ptr(), fopen_mode.as_ptr()) };
    let screen = newterm(None, file, file);
    set_term(screen);

    start_color();
    init_pair(REGULAR_PAIR, COLOR_WHITE, COLOR_BLACK);
    init_pair(CURSOR_PAIR, COLOR_BLACK, COLOR_WHITE);
    init_pair(MATCH_PAIR, COLOR_YELLOW, COLOR_BLACK);
    init_pair(MATCH_CURSOR_PAIR, COLOR_RED, COLOR_WHITE);

    let mut quit = false;
    while !quit {
        let mut cmdline = profile.cmds[profile.current_cmd].clone();
        for (i, (start, end)) in lines[cursor_y].caps.iter().enumerate() {
            cmdline = cmdline.replace(
                format!("\\{}", i + 1).as_str(),
                lines[cursor_y]
                    .text.get(*start..*end)
                    .unwrap_or(""))
        }

        erase();
        render_list(&lines, cursor_y, cursor_x);
        render_status(&cmdline);
        refresh();
        match getch() as u8 as char {
            's' if cursor_y + 1 < lines.len() => cursor_y += 1,
            'w' if cursor_y > 0               => cursor_y -= 1,
            'd'                               => cursor_x += 1,
            'a' if cursor_x > 0               => cursor_x -= 1,
            '\n' => {
                endwin();
                Command::new("sh")
                    .stdin(File::open("/dev/tty")?)
                    .arg("-c")
                    .arg(cmdline)
                    .spawn()?
                    .wait_with_output()?;
            }
            'q' => quit = true,
            _ => {}
        }
    }

    // TODO(#21): if application crashes it does not finalize the terminal
    endwin();
    Ok(())
}
