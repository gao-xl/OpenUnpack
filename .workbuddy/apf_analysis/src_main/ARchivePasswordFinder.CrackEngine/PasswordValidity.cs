using System;

namespace ARchivePasswordFinder.CrackEngine;

[Flags]
public enum PasswordValidity
{
	Invalid = 0,
	UserPasswordIsValid = 1,
	OwnerPasswordIsValid = 2
}
