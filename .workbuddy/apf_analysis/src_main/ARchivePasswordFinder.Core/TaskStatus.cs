namespace ARchivePasswordFinder.Core;

public enum TaskStatus
{
	Open,
	Done,
	Working,
	Error,
	Warning,
	CloudPwdNotFund,
	CloudPwdFunded,
	CloudPwdStart,
	CloudPwdNotEnough,
	WorkingWithForceCrack,
	ForceCracking,
	ForceCrackDone
}
