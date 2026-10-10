using System;

namespace ARchivePasswordFinder.CrackEngine;

[Flags]
public enum Charset
{
	None = 0,
	UpperCaseLetters = 1,
	LowerCaseLetters = 2,
	PunctuationMarks = 4,
	Digits = 8,
	Space = 0x10,
	All = UpperCaseLetters | LowerCaseLetters | PunctuationMarks | Digits | Space
}
