using System;
using System.Collections.Generic;
using System.Linq.Expressions;
using System.Runtime.CompilerServices;
using LiteDB;

namespace APF.Data;

public class LiteDbHelper<Entity> : LiteDbBase
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public Guid Insert(Entity record)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public int InsertBulk(IEnumerable<Entity> record)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public bool Update(Entity record)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public bool InsertUpdate(Entity record)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public IEnumerable<Entity> Find(Expression<Func<Entity, bool>> predicat)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public bool DeleteById<T>(T identifier)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public int DeleteByLinq(Expression<Func<Entity, bool>> func)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ILiteCollection<Entity> GetCollection()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void DropCollection()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void LoadEntityMapper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private bool RecordExist(ILiteCollection<Entity> collection, string field, BsonValue value)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public LiteDbHelper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static LiteDbHelper()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
