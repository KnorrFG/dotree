This is dotree, a small little program that reads a config file like this (following 
platform standard, under linux it will look at `~/.config/dotree.dt`):

```
menu root {
	g: git
	m: misc
}

menu git {
	s: "git status"
	d: "git diff"
	c: "git commit"
	am: "git commit --amend --no-edit"
	aam: "git commit -a --amend --no-edit"
	ca: "git commit -a"
	b: "git switch $(git branch | fzf)"
}

menu misc {
	mn: "mount-netdrives"
	un: "unmount-netdrives"
	cv: "connect-vpn"
}
```

and presents you with the options to execute the commands configured in the file
by typing the configured key. For Example: with the given config file above, I could 
start dotree by typing dt (after it was installed), and then insert gb while dotree is
running to execute `git switch $(git branch | fzf)` in bash. 

Alternativly you can also do that by entering `dt gb`. If you provide an argument, it's
characters will be processed as if you typed them when the program is running.

## Roadmap

The following features are planned:

- Color coding to show typed keys, and an error message when a sequence is invalid
- A configurable default shell
- Asking inputs from the user to insert them into commands
- A parameter to use an alternative config file

## Installation

For now, you will have to either clone the repo, and run `cargo install --path <repo-path>`
or `cargo install --git https://github.com/knorrfg/dotree`.
