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
	am: "amend" - "git commit --amend --no-edit"
	aam: "all amend" - "git commit -a --amend --no-edit"
	ca: "git commit -a"
	b: "git switch $(git branch | fzf)"
	w: cmd {
		vars output_dir, branch
		"add worktree" - !"git worktree add -b "$branch" "$output_dir""!
	}
}

menu misc {
	mn: "mount-netdrives"
	un: "unmount-netdrives"
	cv: "connect-vpn"
}
```

and presents you with the options to execute the commands configured in the file
by typing the configured key. For Example: with the given config file above, I could 
start dotree by typing `dt` (after it was installed), and then type `gb` while dotree is
running to execute `git switch $(git branch | fzf)` in bash. 

Alternativly you can also do that by entering `dt gb`. If you provide an argument, it's
characters will be processed as if you typed them when the program is running.

A command can either be declared as quick command, i.e. a string that contains bash code,
optionally with another string and a `-` in front of it, to have a name displayed in place
of the bash code, or as command via the `cmd` keyword, which allows for the additional
definition of variables that will be queried and then passed as env vars to the bash invocation.

An alternate form of strings are protected strings: `!"<content>"!`, in which case you can use 
`"` freely within the string. and in case you even need `!"` in a string, you can add any
characters between the `!` and the `"`. The characters are not mirrored on the closing 
delimiter. So `!ab"<content>"ab!` is valid, but ~`!ab"<content>"ba!`~ is not.

## Roadmap

The following features are planned:

- A configurable default shell
- A flag to search upwards from the current working directory for a config file
- Different types of auto completion for querying variables
- insert commands, which will only insert the result into bash instead of executing it.
- repeatable commands, usefull for brightnessctl _ or +

## Installation

For now, you will have to either clone the repo, and run `cargo install --path <repo-path>`
or `cargo install --git https://github.com/knorrfg/dotree`.
